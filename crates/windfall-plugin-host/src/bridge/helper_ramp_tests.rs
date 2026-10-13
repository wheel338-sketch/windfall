//! Non-ignored tests of the exact helper owner audio path. The integrator
//! supplies WINDFALL_BRIDGE_FIXTURE; these tests never start nested Cargo.
use super::*;
use crate::{FIRST_NOTE_INSTANCE_ID, PluginNoteControls};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, sync::atomic::{AtomicU64, Ordering}};
use windfall_dsp::ProjectPanLaw as Law;

thread_local! {
    static WATCH: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
fn count() {
    if WATCH.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}
// SAFETY: forwards the allocator contract unchanged; counting is thread local.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { count(); unsafe { System.alloc(layout) } }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { count(); unsafe { System.alloc_zeroed(layout) } }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { count(); unsafe { System.dealloc(pointer, layout) } }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 { count(); unsafe { System.realloc(pointer, layout, size) } }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn guarded(work: impl FnOnce()) -> usize {
    CALLS.set(0); WATCH.set(true); work(); WATCH.set(false); CALLS.get()
}

#[test]
fn helper_project_laws_expression_ramps_and_parameter_events_measure_pcm_without_allocator_calls() {
    for format in ["clap", "vst3"] {
        for law in [Law::Balance, Law::ConstantPower, Law::Compromise, Law::Linear] {
            static SERIAL: AtomicU64 = AtomicU64::new(1);
            let folder = std::env::temp_dir().join(format!("windfall-r6-ramp-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&folder).unwrap();
            let path = folder.join(format!("fixture.{format}"));
            std::fs::copy(std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("explicit prebuilt fixture"), &path).unwrap();
            let host = PluginHost::windfall();
            let module = host.load(&path).unwrap();
            let id = if format == "clap" { "org.windfall.test.sine" } else { "0000000000000000000000000000001B" };
            let mut instance = module.create(id).unwrap();
            let mut reference_instance = module.create(id).unwrap();
            let mut reference = reference_instance.prepare_instrument(48_000.0, 64).unwrap();
            let mut processor = instance.activate(48_000.0, 64).unwrap();
            processor.set_realtime(false);
            let region = Region::initialize(super::super::slots::LocalWords::new(), Config {
                identity: Identity { session: 1, token: 2, revision: 3, binding: 4 },
                sample_rate: 48_000, block: 64, native_latency: processor.latency_samples() as usize, kind: Kind::Instrument,
            }).unwrap();
            let spec = instance.params().iter().find(|parameter| !parameter.read_only).map(|parameter| ParameterSpec {
                id: parameter.id, min: parameter.min, max: parameter.max, value: parameter.default,
                read_only: false, stepped: parameter.stepped,
            }).unwrap();
            let mut native = Native {
                expression_ramps: crate::project_expression::Bank::new(48_000.0),
                wrapper: Default::default(), processor: Some(processor), instance, region,
                parameters: vec![spec], host_values: BTreeMap::from([(spec.id, spec.value)]),
                uncertain: UncertainParameters::new(), notes_uncertain: false, sequence: None,
                epoch: 1, processed_generation: 0, processed_epoch: 0, offline: true, dirty: false,
            };
            let mut input = InputBlock::new();
            input.project_law = crate::project_expression::encode_law(Some(law));
            input.parameter_count = 1;
            input.parameters[0] = Parameter { id: spec.id, value: spec.value };
            input.control_start = 1;
            input.control_end = 1;
            let controls = PluginNoteControls { volume: 1.0, pan: 0.65, pitch: 69.0, modulation_x: 0.5, modulation_y: 0.5 };
            let note = super::super::slots::NoteSnapshot {
                id: FIRST_NOTE_INSTANCE_ID, key: 69, channel: 3, velocity: 0.4,
                controls, expression: Some(controls.mapped(69)),
            };
            input.instances[0] = note;
            input.instance_count = 1;
            assert!(reference.note_on_instance(note.id, note.key, note.channel, note.velocity, PluginNoteControls { pan: 0.0, ..controls }));
            let mut output = OutputBlock::silent();
            let mut baseline = Vec::new();
            let mut actual = Vec::new();
            for sequence in 0..16 {
                input.event_count = 0;
                input.law_event_count = 0;
                if sequence == 8 {
                    // Deliberately not a block boundary: the command is on frame 13.
                    input.law_events[0] = (13, crate::project_expression::encode_law(Some(Law::Linear)));
                    input.law_event_count = 1;
                }
                if sequence > 8 { input.project_law = crate::project_expression::encode_law(Some(Law::Linear)); }
                if sequence == 12 {
                    input.events[0] = HostEvent::NoteExpression { time: 7, id: note.id, key: 69, channel: 3, kind: crate::NoteExpressionKind::Volume, value: 0.25 };
                    input.event_count = 1;
                }
                assert_eq!(guarded(|| native.process(sequence, &input, &mut output).unwrap()), 0, "{format} {law:?} helper allocator");
                let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                reference.process(&mut left, &mut right);
                baseline.extend(left.into_iter().zip(right).map(|(left, right)| [left, right]));
                actual.extend(output.left[..64].iter().zip(&output.right[..64]).map(|(left, right)| [*left, *right]));
            }
            assert!(actual.iter().any(|frame| frame[0].abs() > 0.001));
            let latency = native.region.config.native_latency;
            let from = law.gains(0.65);
            let to = Law::Linear.gains(0.65);
            for frame in latency..actual.len() {
                let clock = frame - latency;
                let law_part = (clock.saturating_sub(8 * 64 + 13) as f32 / 240.0).min(1.0);
                let volume_part = (clock.saturating_sub(12 * 64 + 7) as f32 / 240.0).min(1.0);
                let volume = 1.0 - 0.75 * volume_part;
                for side in 0..2 {
                    let a = [from.0, from.1][side];
                    let b = [to.0, to.1][side];
                    let expected = baseline[frame][side] * (a + (b - a) * law_part) * volume;
                    assert!((actual[frame][side] - expected).abs() < 2e-6, "{format} {law:?} frame={frame} side={side}");
                }
            }
            // A same-valued timed parameter still exercises uncertainty insertion
            // and retirement; neither may allocate or free on the owner audio path.
            input.events[0] = HostEvent::Param { time: 17, id: spec.id, value: spec.value };
            input.event_count = 1;
            assert_eq!(guarded(|| native.process(16, &input, &mut output).unwrap()), 0);
            input.event_count = 0;
            input.parameters[0].value = spec.value * 0.5;
            assert_eq!(guarded(|| native.process(17, &input, &mut output).unwrap()), 0,
                "changed parameter snapshot recovery cannot re-activate or allocate");
            assert_eq!(native.host_values[&spec.id], spec.value * 0.5);
            reference_instance.release_instrument(reference).unwrap();
            drop(native); drop(reference_instance); drop(module); drop(host);
            std::fs::remove_file(path).unwrap(); std::fs::remove_dir(folder).unwrap();
        }
    }
}
