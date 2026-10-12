//! The audio path must not touch the allocator.
//!
//! The host's own code is what is being measured. The test plugins are
//! written not to allocate while they process either, so every call counted
//! here would be one of the host's.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::{GAIN, MIDI_SINE, MONO, SIDECHAIN, SINE, SWAP, create, gain, signal};
use windfall_plugin_host::{HostEvent, Transport};

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every call made on a thread while that
/// thread is inside [`allocator_calls`].
struct CountingAllocator;

fn count() {
    if WATCHING.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}

// SAFETY: every method forwards to the system allocator unchanged. Counting
// reads and writes thread-local cells and nothing else.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Runs `work` and returns how many times it allocated, reallocated or
/// freed memory.
fn allocator_calls(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    WATCHING.set(true);
    work();
    WATCHING.set(false);
    CALLS.get()
}

const RATE: f64 = 48_000.0;

#[test]
fn threaded_submission_reset_stop_and_ownership_return_allocate_and_free_nothing_on_callback() {
    use windfall_plugin_host::wrapper::{ProcessingMode, WrapperOptions};
    let (_module, mut instance) = create(GAIN);
    let mut processor = instance.activate_with_options(RATE, 64, &WrapperOptions { processing: ProcessingMode::Threaded, ..Default::default() }).unwrap();
    assert_eq!(processor.latency_samples(), 128);
    let mut left = [1.0; 64]; let mut right = left;
    assert_eq!(allocator_calls(|| {
        assert!(processor.set_param(7, 7, 0.25));
        processor.process(&mut left, &mut right);
        processor.reset();
        processor.stop();
    }), 0);
    assert_eq!(left, [0.0; 64]);
    instance.deactivate(processor).unwrap();
}

#[test]
fn r4_inactive_and_timed_vst3_points_reach_native_at_their_frames() {
    let path = common::plugin_file("vst3-point-expansion", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    assert!(instance.set_param(7, 0.25));
    let mut processor = instance.activate(RATE, 1025).unwrap();
    let mut left = [1.0; 1025];
    let mut right = left;
    let calls = allocator_calls(|| {
        for time in 1..=windfall_plugin_host::EVENT_CAPACITY as u32 {
            assert!(processor.set_param(time, 7, if time == 1024 { 0.75 } else { 0.5 }));
        }
        processor.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert_eq!(left[0], 0.5, "inactive point must process at frame zero");
    assert!(left[1..1024].iter().all(|value| *value == 1.0));
    assert_eq!(
        left[1024], 1.5,
        "the last admitted timed point was lost in native translation"
    );
    assert_eq!(instance.param_value(7), Some(0.75));
    assert_eq!(processor.health().dropped_events, 0);
    left.fill(1.0);
    right.fill(1.0);
    assert_eq!(
        allocator_calls(|| {
            processor.process(&mut left, &mut right);
        }),
        0
    );
    assert!(left.iter().all(|value| *value == 1.5));
    instance.deactivate(processor).unwrap();
}

#[test]
fn r4_disjoint_vst3_pending_editor_and_timed_controls_process_truthfully() {
    vst3_disjoint_points(1);
}

#[test]
fn r4_full_editor_queue_and_other_native_point_sources_fit_without_allocating() {
    vst3_disjoint_points(4096);
}

fn vst3_disjoint_points(editor_points: usize) {
    let path = common::plugin_file("vst3-three-point-sources", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[9].id).unwrap();
    assert!(instance.set_param(8, 0.25));
    let mut processor = instance.activate(RATE, 1025).unwrap();
    for _ in 0..editor_points {
        instance.param_text(7, 0.9375).unwrap(); // queues an editor point on ID 9
    }
    let mut left = [1.0; 1025];
    let mut right = left;
    let calls = allocator_calls(|| {
        for time in 1..=windfall_plugin_host::EVENT_CAPACITY as u32 {
            assert!(processor.set_param(time, 7, if time == 1024 { 0.75 } else { 0.5 }));
        }
        processor.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert!(
        right.iter().all(|value| *value == 1.0),
        "disjoint pending/editor points must actually process"
    );
    assert_eq!(
        left[1024], 1.5,
        "desktop point was admitted but not applied by the native component"
    );
    assert_eq!(instance.param_value(7), Some(0.75));
    assert_eq!(instance.param_value(8), Some(0.25));
    assert_eq!(instance.param_value(9), Some(0.75));
    assert_eq!(processor.health().dropped_events, 0);
    left.fill(1.0);
    right.fill(1.0);
    assert_eq!(
        allocator_calls(|| {
            processor.process(&mut left, &mut right);
        }),
        0
    );
    assert!(left.iter().all(|value| *value == 1.5));
    assert!(right.iter().all(|value| *value == 1.0));
    instance.deactivate(processor).unwrap();
}

#[test]
fn r4_failed_native_process_never_publishes_control_readback_and_retains_final_intent() {
    let path = common::plugin_file("vst3-failed-points", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[4].id).unwrap();
    let mut processor = instance.activate(RATE, 64).unwrap();
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        allocator_calls(|| {
            assert!(processor.set_param(1, 7, 0.25));
            assert!(processor.set_param(63, 7, 0.75));
            assert_eq!(
                processor.process(&mut left, &mut right),
                windfall_plugin_host::ProcessStatus::Failed
            );
        }),
        0
    );
    assert_eq!(
        instance.param_value(7),
        Some(0.5),
        "a failed process is not application"
    );
    assert_eq!(processor.health().dropped_events, 0);
    instance.deactivate(processor).unwrap();
    // Owner return applies retained intent to the inactive controller, never
    // claiming that the failed component processed it.
    assert_eq!(instance.param_value(7), Some(0.75));
}

#[test]
fn saturated_adapter_releases_are_delivered_without_allocator_calls() {
    for id in [SINE, MIDI_SINE] {
        let (_module, mut instance) = create(id);
        let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
        let mut left = [0.0; 256];
        let mut right = left;
        instrument.process(&mut left, &mut right);
        let calls = allocator_calls(|| {
            for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
                instrument.note_on(64, 1.0);
            }
            instrument.processor().process(&mut [], &mut []);
            instrument.all_notes_off();
            instrument.process(&mut left, &mut right);
        });
        assert_eq!(calls, 0, "{id}");
        assert!(
            left.iter().all(|sample| *sample == 0.0),
            "{id}: queued notes sounded after panic"
        );
        assert_eq!(instrument.active_voices(), 0);
        instance.release_instrument(instrument).unwrap();
    }
}

#[test]
fn saturated_adapter_note_off_preserves_an_independent_key_without_allocating() {
    for id in [SINE, MIDI_SINE] {
        let (_module, mut instance) = create(id);
        let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
        let mut left = [0.0; 256];
        let mut right = left;
        instrument.note_on(69, 1.0); // independently held UI key
        instrument.note_on(64, 1.0);
        instrument.process(&mut left, &mut right);
        let calls = allocator_calls(|| {
            for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
                assert!(instrument.processor().set_param(0, common::SINE_LEVEL, 0.5));
            }
            instrument.note_off(64);
            instrument.process(&mut left, &mut right);
        });
        assert_eq!(calls, 0, "{id}");
        assert_eq!(instrument.active_voices(), 1);
        assert!(
            left.iter().any(|sample| sample.abs() > 0.01),
            "{id}: independent key was silenced"
        );
        instrument.note_off(69);
        instrument.process(&mut left, &mut right);
        instrument.process(&mut left, &mut right); // drain the fixture's latency
        assert!(
            left.iter().all(|sample| *sample == 0.0),
            "{id}: released key is still sounding"
        );
        instance.release_instrument(instrument).unwrap();
    }
}

#[test]
fn saturated_release_storm_stays_bounded_and_preserves_accepted_parameters() {
    for id in [SINE, MIDI_SINE] {
        let (_module, mut instance) = create(id);
        let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
        let mut left = [0.0; 256];
        let mut right = left;
        for key in 0..128 {
            instrument.note_on(key, 1.0);
        }
        instrument.process(&mut left, &mut right);
        let calls = allocator_calls(|| {
            for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
                assert!(
                    instrument
                        .processor()
                        .set_param(0, common::SINE_LEVEL, 0.75)
                );
            }
            for _ in 0..8 {
                for key in 0..128 {
                    instrument.note_off(key);
                }
                instrument.all_notes_off();
            }
            assert!(
                !instrument
                    .processor()
                    .set_param(0, common::SINE_LEVEL, 0.25)
            );
            instrument.process(&mut left, &mut right);
            instrument.process(&mut left, &mut right);
        });
        assert_eq!(calls, 0, "{id}");
        assert_eq!(instrument.health().dropped_events, 1, "{id}");
        assert_eq!(instrument.active_voices(), 0);
        assert!(left.iter().all(|sample| *sample == 0.0), "{id}");
        assert_eq!(instance.param_value(common::SINE_LEVEL), Some(0.75));
        instance.release_instrument(instrument).unwrap();
    }
}

#[test]
fn saturated_releases_keep_order_after_intervening_same_frame_notes() {
    for id in [SINE, MIDI_SINE] {
        for panic in [false, true] {
            let (_module, mut instance) = create(id);
            let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
            let mut left = [0.0; 256];
            let mut right = left;
            instrument.process(&mut left, &mut right);
            let calls = allocator_calls(|| {
                if panic {
                    instrument.all_notes_off();
                } else {
                    instrument.note_off(64);
                }
                instrument.note_on(64, 1.0);
                for _ in 0..windfall_plugin_host::EVENT_CAPACITY - 2 {
                    assert!(instrument.processor().set_param(0, common::SINE_LEVEL, 0.5));
                }
                // The earlier release cannot cover the intervening note-on.
                if panic {
                    instrument.all_notes_off();
                } else {
                    instrument.note_off(64);
                }
                instrument.process(&mut left, &mut right);
            });
            assert_eq!(calls, 0, "{id}, panic={panic}");
            assert_eq!(instrument.health().dropped_events, 0);
            assert_eq!(instrument.active_voices(), 0);
            assert!(
                left.iter().all(|sample| *sample == 0.0),
                "{id}, panic={panic}"
            );
            instance.release_instrument(instrument).unwrap();
        }
    }
}

#[test]
fn accepted_main_thread_parameter_burst_waits_for_bounded_audio_admission() {
    let (_module, mut instance) = create(GAIN);
    let mut processor = instance.activate(RATE, 64).unwrap();
    for _ in 0..4095 {
        assert!(instance.set_param(gain::GAIN, 0.25));
    }
    assert!(instance.set_param(gain::GAIN, 0.75));
    let mut left = [1.0; 64];
    let mut right = left;
    let calls = allocator_calls(|| {
        processor.process(&mut [], &mut []);
        for _ in 0..4 {
            left.fill(1.0);
            right.fill(1.0);
            processor.process(&mut left, &mut right);
        }
    });
    assert_eq!(calls, 0);
    assert_eq!(left, [0.75; 64]);
    assert_eq!(processor.health().dropped_events, 0);
    instance.deactivate(processor).unwrap();
}

#[test]
fn vst3_ownership_boundaries_move_adapters_without_allocator_calls() {
    use windfall_plugin_host::ownership::exchange;
    let path = common::plugin_file("vst3-ownership", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    let adapter = instance.prepare_effect(RATE as f32, 64).unwrap();
    let (mut owner, mut audio) = exchange(adapter);
    let mut left = [0.25; 64];
    let mut right = left;
    for _ in 0..20 {
        owner.request();
        assert_eq!(
            allocator_calls(|| {
                audio.boundary();
            }),
            0
        );
        assert!(audio.current().is_none());
        let adapter = owner.take_returned().unwrap();
        instance.release_effect(adapter).unwrap();
        let state = instance.save_state().unwrap();
        instance.load_state(&state).unwrap();
        owner
            .resume(instance.prepare_effect(RATE as f32, 64).unwrap())
            .ok()
            .unwrap();
        assert_eq!(
            allocator_calls(|| {
                audio.boundary();
                audio.current_mut().unwrap().process(&mut left, &mut right);
            }),
            0
        );
    }
    instance.release_effect(audio.retire().unwrap()).unwrap();
}

#[test]
fn vst3_reset_and_saturated_release_keep_every_admitted_note_off() {
    let path = common::plugin_file("vst3-saturated-release", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[2].id).unwrap();
    let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
    let mut left = [0.0; 64];
    let mut right = left;
    for first in [0, 8] {
        for channel in first..first + 8 {
            for key in 0..128 {
                assert!(instrument.processor().push_event(HostEvent::NoteOn {
                    time: 0,
                    key,
                    channel,
                    velocity: 1.0
                }));
            }
        }
        instrument.process(&mut left, &mut right);
    }
    let calls = allocator_calls(|| {
        instrument.processor().reset();
        for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
            assert!(instrument.note_on(64, 1.0));
        }
        assert!(instrument.note_off(64));
        instrument.process(&mut left, &mut right);
        instrument.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert!(
        left.iter().all(|sample| *sample == 0.0),
        "the reserved release was lost after reset expansion"
    );
    assert_eq!(instrument.health().dropped_events, 0);
    instance.release_instrument(instrument).unwrap();
}

#[test]
fn runtime_repair_vst3_ordinary_and_reserved_panics_release_all_channels() {
    let path = common::plugin_file("vst3-panic-expansion", "fixture.vst3");
    let module = windfall_plugin_host::PluginHost::windfall()
        .load(&path)
        .unwrap();
    let mut instance = module.create(&module.descriptors()[2].id).unwrap();
    let mut instrument = instance.prepare_instrument(RATE as f32, 64).unwrap();
    let mut left = [0.0; 64];
    let mut right = left;
    for first in [0, 8] {
        for channel in first..first + 8 {
            for key in 0..128 {
                assert!(instrument.processor().push_event(HostEvent::NoteOn {
                    time: 0,
                    key,
                    channel,
                    velocity: 1.0
                }));
            }
        }
        instrument.process(&mut left, &mut right);
    }
    assert!(left.iter().any(|value| value.abs() > 0.01));
    let calls = allocator_calls(|| {
        assert!(instrument.processor().all_notes_off(0));
        for index in 0..windfall_plugin_host::EVENT_CAPACITY - 1 {
            assert!(instrument.processor().push_event(HostEvent::NoteOn {
                time: 0,
                key: (index % 128) as u8,
                channel: (index / 128) as u8,
                velocity: 1.0
            }));
        }
        assert!(instrument.all_notes_off());
        instrument.process(&mut left, &mut right);
        assert!(instrument.all_notes_off());
        instrument.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert_eq!(
        instrument.health().dropped_events,
        0,
        "every admitted translated release must fit"
    );
    assert!(
        left.iter().all(|value| *value == 0.0),
        "native polyphonic notes survived both panics"
    );
    instance.release_instrument(instrument).unwrap();
}

#[test]
fn vst3_effect_and_instrument_callbacks_allocate_and_free_nothing() {
    for index in [0, 2, 3] {
        let path = common::plugin_file("vst3", "fixture.vst3");
        let module = windfall_plugin_host::PluginHost::windfall()
            .load(&path)
            .unwrap();
        let mut instance = module.create(&module.descriptors()[index].id).unwrap();
        let mut p = instance.activate(RATE, 64).unwrap();
        p.set_realtime(false);
        let mut left = [0.25; 256];
        let mut right = [0.5; 256];
        let calls = allocator_calls(|| {
            for _ in 0..20 {
                p.note_on(3, 60, 0.5);
                p.note_off(125, 60);
                p.set_param(80, 7, 0.75);
                p.process(&mut left, &mut right);
            }
            p.reset();
            p.stop();
            p.all_notes_off(0);
            p.process(&mut left, &mut right);
        });
        assert_eq!(calls, 0, "VST3 fixture {index}");
        assert_eq!(p.health().dropped_events, 0);
        instance.deactivate(p).unwrap();
    }
}

#[test]
fn the_counter_counts() {
    assert_eq!(
        allocator_calls(|| drop(std::hint::black_box(Box::new(1_u8)))),
        2
    );
}

#[test]
fn an_effect_processes_without_allocating() {
    let (_module, mut instance) = create(GAIN);
    let mut processor = instance.activate(RATE, 256).unwrap();
    let (mut left, mut right) = signal(1000);
    // A parameter change that crosses from the main thread.
    instance.set_param(gain::MODE, 1.0);

    let calls = allocator_calls(|| {
        for block in 0..50 {
            // Events of every kind, a new transport, and a block longer
            // than the plugin's maximum, which is split.
            processor.set_param(block, gain::GAIN, 0.5 + f64::from(block) * 0.01);
            processor.set_param(700, gain::GAIN, 1.0);
            // The key that makes the plugin send a gesture back.
            processor.note_on(10, 127, 0.25);
            processor.push_event(HostEvent::AllNotesOff { time: 20 });
            processor.set_transport(Transport {
                playing: true,
                tempo_bpm: 128.0,
                position_beats: f64::from(block),
                ..Transport::default()
            });
            processor.process(&mut left, &mut right);
            let _ = processor.latency_samples();
            let _ = processor.tail_samples();
            let _ = processor.health();
        }
        processor.reset();
        processor.stop();
        processor.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert_eq!(processor.health().dropped_events, 0);
    instance.deactivate(processor).unwrap();
}

#[test]
fn an_instrument_processes_without_allocating() {
    for id in [SINE, MIDI_SINE] {
        let (_module, mut instance) = create(id);
        let mut processor = instance.activate(RATE, 512).unwrap();
        let mut left = vec![0.0_f32; 512];
        let mut right = vec![0.0_f32; 512];
        let calls = allocator_calls(|| {
            for block in 0..50_u8 {
                processor.note_on(3, 40 + block, 0.5);
                processor.note_off(400, 40 + block);
                processor.set_param(100, common::SINE_LEVEL, 0.25);
                processor.process(&mut left, &mut right);
            }
            processor.all_notes_off(0);
            processor.process(&mut left, &mut right);
        });
        assert_eq!(calls, 0, "{id}");
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn unusual_port_layouts_process_without_allocating() {
    for id in [SWAP, SIDECHAIN, MONO] {
        let (_module, mut instance) = create(id);
        let mut processor = instance.activate(RATE, 128).unwrap();
        let (mut left, mut right) = signal(300);
        let calls = allocator_calls(|| {
            for _ in 0..20 {
                processor.process(&mut left, &mut right);
            }
        });
        assert_eq!(calls, 0, "{id}");
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn a_full_event_queue_drops_events_without_allocating() {
    let (_module, mut instance) = create(GAIN);
    let mut processor = instance.activate(RATE, 256).unwrap();
    let (mut left, mut right) = signal(256);
    let calls = allocator_calls(|| {
        for index in 0..3_000_u32 {
            processor.set_param(index % 256, gain::GAIN, 1.0);
        }
        processor.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert_eq!(
        processor.health().dropped_events as usize,
        3_000 - windfall_plugin_host::EVENT_CAPACITY
    );
    instance.deactivate(processor).unwrap();
}

#[test]
fn the_adapters_process_without_allocating() {
    let (_module, mut instance) = create(GAIN);
    let mut effect = instance.prepare_effect(48_000.0, 256).unwrap();
    let (mut left, mut right) = signal(256);
    let calls = allocator_calls(|| {
        for block in 0..20 {
            effect.set_param(0, 0.5);
            effect.set_tempo(100.0 + block as f32);
            effect.process(&mut left, &mut right);
            let _ = (
                effect.latency_samples(),
                effect.tail_samples(),
                effect.gap_samples(),
            );
        }
        effect.reset();
    });
    assert_eq!(calls, 0);
    instance.release_effect(effect).unwrap();

    let (_sine_module, mut sine) = create(SINE);
    let mut instrument = sine.prepare_instrument(48_000.0, 256).unwrap();
    let calls = allocator_calls(|| {
        for block in 0..20_u8 {
            instrument.note_on(60 + block, 0.8);
            instrument.process(&mut left, &mut right);
            instrument.note_off(60 + block);
            instrument.process(&mut left, &mut right);
            let _ = (instrument.active_voices(), instrument.tail_samples());
        }
        instrument.all_notes_off();
        instrument.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    sine.release_instrument(instrument).unwrap();
}

#[test]
fn translated_midi_panics_fit_the_bounded_list_without_allocating() {
    let (_module, mut instance) = create(MIDI_SINE);
    let mut processor = instance.activate(RATE, 64).unwrap();
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    let calls = allocator_calls(|| {
        for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
            assert!(processor.all_notes_off(0));
        }
        processor.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
    assert_eq!(processor.health().dropped_events, 0);
    instance.deactivate(processor).unwrap();
}
