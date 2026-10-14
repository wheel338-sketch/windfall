//! An independent open fixture exercises the actual SDK process ABI.
#![cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod common;
use windfall_plugin_host::{EditorError, EditorOptions, PluginHost, ProcessStatus, Transport};
fn create(
    index: usize,
) -> (
    windfall_plugin_host::PluginModule,
    windfall_plugin_host::PluginInstance,
) {
    let path = common::plugin_file("vst3", "fixture.vst3");
    let module = PluginHost::windfall().load(&path).unwrap();
    let instance = module.create(&module.descriptors()[index].id).unwrap();
    (module, instance)
}

#[test]
fn independent_vst3_no_editor_class_keeps_explicit_refusal() {
    let (_module, mut instance) = create(3);
    assert!(!instance.has_editor());
    assert_eq!(
        instance.open_editor(&EditorOptions::default()),
        // Windows and Linux (X11) host VST3 editors, so a plugin without
        // an editor view is refused as having none.
        Err(if cfg!(any(windows, target_os = "linux")) {
            EditorError::NoEditor
        } else {
            EditorError::UnsupportedPlatform
        })
    );
}

#[test]
fn native_numeric_expression_controls_distinct_overlapping_note_identities() {
    use windfall_plugin_host::{FIRST_NOTE_INSTANCE_ID, PluginNoteControls};
    let (_module, mut instance) = create(23);
    let mut instrument = instance.prepare_instrument(48_000.0, 64).unwrap();
    assert!(instrument.supports_note_instances());
    assert!(instrument.supports_note_pitch());
    assert_eq!(instrument.processor().note_expression_mask(2), 0x36);
    assert_eq!(instrument.processor().note_expression_mask(15), 0);
    assert_eq!(instrument.latency_samples(), 0);
    let a = PluginNoteControls {
        volume: 1.0,
        pan: -0.5,
        pitch: 69.5,
        modulation_x: 0.25,
        modulation_y: 0.75,
    };
    let b = PluginNoteControls {
        volume: 1.0,
        pan: 0.5,
        pitch: 75.0,
        modulation_x: 0.5,
        modulation_y: 0.25,
    };
    let id = FIRST_NOTE_INSTANCE_ID;
    assert!(instrument.note_on_instance(id, 69, 2, 0.5, a));
    assert!(instrument.note_on_instance(id + 1, 69, 2, 0.25, b));
    let mut left = [0.0; 64];
    let mut right = left;
    instrument.process(&mut left, &mut right);
    let mark_a = 0.5_f64 * (1.0 + 0.5 / 12.0 + 0.75 / 4.0 + 0.25 / 8.0);
    let mark_b = 0.25_f64 * (1.0 + 6.0 / 12.0 + 0.25 / 4.0 + 0.5 / 8.0);
    for (l, r) in left.iter().zip(&right) {
        assert!((f64::from(*l) - (mark_a * 0.75 + mark_b * 0.25)).abs() < 1e-6);
        assert!((f64::from(*r) - (mark_a * 0.25 + mark_b * 0.75)).abs() < 1e-6);
    }
    assert!(instrument.note_off_instance(id, 0.375));
    instrument.process(&mut left, &mut right);
    assert!(
        left.iter()
            .all(|value| (f64::from(*value) - mark_b * 0.25).abs() < 1e-6)
    );
    assert!(
        right
            .iter()
            .all(|value| (f64::from(*value) - mark_b * 0.75).abs() < 1e-6)
    );
    assert_eq!(instrument.active_voices(), 1);
    assert_eq!(instrument.health().dropped_events, 0);
    instance.release_instrument(instrument).unwrap();
}

#[test]
fn refusing_native_deactivation_never_proves_inactive_state() {
    let (_module, mut instance) = create(6);
    let processor = instance.activate(48_000.0, 64).unwrap();
    let mut processor = instance.deactivate(processor).unwrap_err().returned;
    assert!(instance.is_active(), "setActive(false) was refused");
    // This asserted that state cannot be saved while the plugin is active.
    // Since 2026-10-13 a save reads state from the running plugin, so that
    // saving while music plays never takes the plugin from the audio. A
    // refused deactivation still proves nothing about an inactive state:
    // what needs one, loading, stays refused.
    let active = instance.save_state().expect("state is read while active");
    assert!(
        instance.load_state(&active).is_err(),
        "loading into an active plugin must remain forbidden"
    );
    processor.set_param(0, 7, 0.25);
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(
        left, [0.5; 64],
        "the same processor can recover after refusal"
    );
    instance.deactivate(processor).unwrap();
    assert!(!instance.is_active());
    instance.save_state().unwrap();
}
#[test]
fn refusing_native_processing_stop_returns_the_running_processor() {
    let (_module, mut instance) = create(6);
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    assert!(processor.set_param(0, 7, 0.75));
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(left, [1.5; 64]);

    let refused = instance.deactivate(processor).unwrap_err();
    assert!(
        refused
            .error
            .to_string()
            .contains("setProcessing(false) refused")
    );
    assert!(instance.is_active());
    // State is read while the plugin stays active (see the test above).
    assert!(instance.save_state().is_ok());
    let mut processor = refused.returned;
    assert!(processor.set_param(0, 7, 0.25));
    left.fill(1.0);
    right.fill(1.0);
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(left, [0.5; 64]);
    instance.deactivate(processor).unwrap();
    assert!(!instance.is_active());
    instance.save_state().unwrap();
}
#[test]
fn runtime_repair_native_deactivation_preset_supersedes_older_queued_controls() {
    let (_module, mut instance) = create(7);
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    assert!(processor.set_param(0, 7, 0.875));
    let mut left = [1.0; 64];
    let mut right = left;
    processor.process(&mut left, &mut right);
    instance.idle(&mut |_| {});
    processor.process(&mut left, &mut right);
    assert_eq!(instance.param_value(7), Some(0.625));
    assert!(processor.set_param(0, 7, 0.8));
    instance.deactivate(processor).unwrap();
    assert_eq!(instance.deactivation_param_value(7), Some(0.375));
    assert_eq!(instance.param_value(7), Some(0.375));
    let state = instance.save_state().unwrap();
    let (_module, mut reopened) = create(7);
    reopened.load_state(&state).unwrap();
    assert_eq!(reopened.param_value(7), Some(0.375));
    // A new explicit inactive edit comes after deactivation and stays valid.
    assert!(instance.set_param(7, 0.9));
    assert_eq!(instance.param_value(7), Some(0.9));
}

#[test]
fn lifecycle_gain_offsets_transport_state_and_restart() {
    let (module, mut instance) = create(0);
    drop(module); // instance retains DLL
    assert_eq!(instance.param_text(7, 0.25).as_deref(), Some("0.50"));
    assert_eq!(instance.param_from_text(7, "0.50"), Some(0.25));
    assert_eq!(instance.has_editor(), cfg!(windows));
    let opened = instance.open_editor(&EditorOptions::default());
    if cfg!(windows) {
        let info = opened.unwrap();
        assert_eq!((info.width, info.height), (320, 200));
        assert!(info.window.is_some());
        instance.close_editor();
    } else if cfg!(target_os = "linux") {
        // The fixture draws an editor on Windows only; the X11 host asks
        // for a view and is given none.
        assert_eq!(opened, Err(EditorError::NoEditor));
    } else {
        assert_eq!(opened, Err(EditorError::UnsupportedPlatform));
    }
    instance.set_param(7, 0.25);
    let state = instance.save_state().unwrap();
    instance.set_param(7, 1.0);
    instance.load_state(&state).unwrap();
    assert_eq!(instance.param_value(7), Some(0.25));
    let mut processor = instance.activate(48_000.0, 32).unwrap();
    // Saving reads state from the active plugin; loading into it is refused.
    assert!(instance.save_state().is_ok());
    assert!(instance.load_state(&state).is_err());
    assert_eq!(processor.latency_samples(), 17);
    assert_eq!(processor.tail_samples(), Some(64));
    processor.set_realtime(false);
    processor.set_transport(Transport {
        playing: true,
        tempo_bpm: 137.0,
        position_beats: 8.0,
        position_seconds: 1.0,
        ..Transport::default()
    });
    processor.set_param(16, 7, 0.75);
    processor.set_param(48, 7, 0.125);
    let mut left = [1.0; 64];
    let mut right = [0.5; 64];
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(&left[..16], &[0.5; 16]);
    assert_eq!(&left[16..48], &[1.5; 32]);
    assert_eq!(&left[48..], &[0.25; 16]);
    assert_eq!(&right[..16], &[0.25; 16]);
    assert_eq!(processor.health().dropped_events, 0);
    assert!((instance.param_value(22).unwrap() - 137.0 / 300.0).abs() < 1e-12);
    assert_eq!(instance.param_value(23), Some(1.0));
    assert!(
        (instance.param_value(24).unwrap() - (8.0 + 32.0 / 48_000.0 * 137.0 / 60.0) / 1000.0).abs()
            < 1e-12
    );
    processor.reset();
    processor.stop();
    instance.deactivate(processor).unwrap();
    assert_eq!(instance.param_value(7), Some(0.125));
    instance.load_state(&state).unwrap();
    let mut second = instance.activate(44_100.0, 64).unwrap();
    second.set_realtime(false);
    left.fill(1.0);
    right.fill(1.0);
    second.process(&mut left, &mut right);
    assert_eq!(left, [0.5; 64]);
    instance.deactivate(second).unwrap();
}

#[test]
fn returning_before_the_next_block_preserves_control_and_scheduled_parameter_edits() {
    let (_module, mut instance) = create(0);
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    // Neither point has reached native process. Main-thread points precede
    // scheduled audio points at an equal frame, just as in normal processing.
    assert!(instance.set_param(7, 0.25));
    assert!(processor.set_param(3, 7, 0.75));
    instance.deactivate(processor).unwrap();
    let state = instance.save_state().unwrap();
    assert_eq!(instance.param_value(7), Some(0.75));
    instance.set_param(7, 0.1);
    instance.load_state(&state).unwrap();
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    let mut left = [1.0; 64];
    let mut right = left;
    processor.process(&mut left, &mut right);
    assert_eq!(left, [1.5; 64]);
    instance.deactivate(processor).unwrap();
}

#[test]
fn dropping_an_instance_with_its_processor_out_keeps_the_plugin_alive() {
    let (module, mut instance) = create(0);
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    processor.set_realtime(false);
    drop(instance);
    drop(module);
    let mut left = [0.25; 64];
    let mut right = [0.5; 64];
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(left, [0.25; 64]);
    processor.stop();
    // API misuse deliberately leaks the COM/module graph rather than
    // terminating beneath an outstanding audio owner.
}

#[test]
fn independent_module_handles_keep_instances_alive_after_one_handle_drops() {
    let path = common::plugin_file("vst3", "fixture.vst3");
    let host = PluginHost::windfall();
    let first = host.load(&path).unwrap();
    let second = host.load(&path).unwrap();
    let mut a = first.create(&first.descriptors()[0].id).unwrap();
    let mut b = second.create(&second.descriptors()[0].id).unwrap();
    drop(first);
    drop(second);
    let mut pa = a.activate(48_000.0, 64).unwrap();
    let mut pb = b.activate(48_000.0, 64).unwrap();
    pa.set_realtime(false);
    pb.set_realtime(false);
    let mut left = [0.25; 64];
    let mut right = [0.5; 64];
    pa.process(&mut left, &mut right);
    a.deactivate(pa).unwrap();
    drop(a);
    assert_eq!(pb.process(&mut left, &mut right), ProcessStatus::Continue);
    assert_eq!(left, [0.25; 64]);
    b.deactivate(pb).unwrap();
}

#[test]
fn malformed_state_lengths_ids_and_values_are_rejected_before_restore() {
    let (_module, mut instance) = create(0);
    instance.set_param(7, 0.25);
    let state = instance.save_state().unwrap();
    for damage in 0..4 {
        let mut bytes = state.as_bytes().to_vec();
        match damage {
            0 => bytes[10..14].copy_from_slice(&u32::MAX.to_le_bytes()), // component length
            1 => bytes[18..22].copy_from_slice(&u32::MAX.to_le_bytes()), // override count
            2 => {
                let end = bytes.len();
                bytes[end - 12..end - 8].copy_from_slice(&u32::MAX.to_le_bytes());
            }
            _ => {
                let end = bytes.len();
                bytes[end - 8..].copy_from_slice(&f64::NAN.to_le_bytes());
            }
        }
        assert!(
            instance
                .load_state(&windfall_plugin_host::PluginState::from_bytes(bytes))
                .is_err()
        );
        assert_eq!(instance.param_value(7), Some(0.25));
    }
}

#[test]
fn vst3_process_failure_disables_further_calls_and_bypasses_the_effect() {
    let (_module, mut instance) = create(4);
    let mut p = instance.activate(48_000.0, 64).unwrap();
    p.set_realtime(false);
    let mut left = [1.0; 64];
    let mut right = [1.0; 64];
    assert_eq!(p.process(&mut left, &mut right), ProcessStatus::Failed);
    assert!(p.health().failed);
    // The common host keeps dry input for separate-buffer effects. On
    // failure it restores that input rather than trusting plugin output.
    assert_eq!(left, [1.0; 64]);
    assert_eq!(right, [1.0; 64]);
    assert_eq!(p.process(&mut left, &mut right), ProcessStatus::Failed);
    instance.deactivate(p).unwrap();
}
#[test]
fn vst3_nonfinite_output_is_scrubbed_before_it_leaves_the_host() {
    let (_module, mut instance) = create(5);
    let mut p = instance.activate(48_000.0, 64).unwrap();
    p.set_realtime(false);
    let mut left = [1.0; 64];
    let mut right = [1.0; 64];
    assert_eq!(p.process(&mut left, &mut right), ProcessStatus::Continue);
    assert_eq!(left, [0.0; 64]);
    assert_eq!(right, [0.0; 64]);
    assert_eq!(p.health().scrubbed_samples, 128);
    instance.deactivate(p).unwrap();
}
#[test]
fn instrument_sample_offsets_all_notes_off_and_adapters() {
    let (_module, mut instance) = create(2);
    let mut p = instance.activate(48_000.0, 64).unwrap();
    p.set_realtime(false);
    let mut left = [0.0; 64];
    let mut right = [0.0; 64];
    p.note_on(8, 60, 0.8);
    p.note_off(48, 60);
    p.process(&mut left, &mut right);
    assert!(left[..8].iter().all(|x| *x == 0.0));
    assert!(left[8..48].iter().any(|x| x.abs() > 0.01));
    assert!(left[48..].iter().all(|x| *x == 0.0));
    assert_eq!(left, right);
    p.note_on(0, 62, 0.5);
    p.all_notes_off(16);
    p.process(&mut left, &mut right);
    assert!(left[16..].iter().all(|x| *x == 0.0));
    instance.deactivate(p).unwrap();
    let mut adapter = instance.prepare_instrument(48_000.0, 64).unwrap();
    adapter.note_on(60, 0.8);
    adapter.process(&mut left, &mut right);
    assert!(left.iter().any(|x| x.abs() > 0.01));
    adapter.all_notes_off();
    adapter.process(&mut left, &mut right);
    assert!(left.iter().all(|x| *x == 0.0));
    instance.release_instrument(adapter).unwrap();
}
#[test]
fn mono_mean_input_duplicates_output_and_effect_adapter() {
    let (_module, mut instance) = create(3);
    let mut adapter = instance.prepare_effect(48_000.0, 64).unwrap();
    let mut left = [1.0; 64];
    let mut right = [-0.5; 64];
    adapter.process(&mut left, &mut right);
    assert_eq!(left, [0.25; 64]);
    assert_eq!(left, right);
    assert!(adapter.set_param(0, 0.25));
    left.fill(1.0);
    right.fill(1.0);
    adapter.process(&mut left, &mut right);
    assert_eq!(left, [0.5; 64]);
    instance.release_effect(adapter).unwrap();
}

#[test]
fn a_live_note_expression_change_is_followed_by_the_running_processor_without_a_restart() {
    use windfall_plugin_host::{NoteExpressionKind as K, PluginNotification};
    // Class 28 withholds per-note pan or volume while it runs and says so
    // with kNoteExpressionChanged alone.
    let both = K::Pan.mask() | K::Volume.mask();
    for follow in [true, false] {
        let (_module, mut instance) = create(28);
        instance.follow_note_expression_changes(follow);
        let processor = instance.activate(48_000.0, 64).unwrap();
        assert_eq!(processor.note_expression_mask(0) & both, both);
        // 31/32 withholds pan, 61/64 volume, 59/64 both, 29/32 neither.
        for (mark, offered) in [
            (0.96875, K::Volume.mask()),
            (0.953125, K::Pan.mask()),
            (0.921875, 0),
            (0.90625, both),
        ] {
            instance.param_text(7, mark);
            let mut restart = false;
            instance.idle(&mut |notification| {
                restart |= matches!(notification, PluginNotification::RestartRequested)
            });
            if follow {
                assert!(!restart, "followed live: no restart is asked for");
                assert_eq!(processor.note_expression_mask(0) & both, offered);
                // Channel 16 offers nothing, whatever the switch says.
                assert_eq!(processor.note_expression_mask(15), 0);
            } else {
                // Not followed: the change is a restart request as before,
                // and the running processor keeps what it was activated with.
                assert!(restart);
                assert_eq!(processor.note_expression_mask(0) & both, both);
            }
        }
        assert!(instance.is_active());
        instance.deactivate(processor).unwrap();
    }
}
