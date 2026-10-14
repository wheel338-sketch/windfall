//! Repository-owned native switches exercise actual inactive re-preparation.
mod common;
use windfall_plugin_host::{PluginHost, PluginNotification, PluginState, ProcessStatus};

#[test]
fn clap_and_vst3_live_switches_readmit_ports_latency_parameters_and_note_capabilities() {
    for format in ["clap", "vst3"] {
        let path = common::plugin_file("dynamic-capabilities", &format!("dynamic.{format}"));
        let module = PluginHost::windfall().load(&path).unwrap();
        let id = if format == "clap" {
            "org.windfall.test.dynamic"
        } else {
            "0000000000000000000000000000001A"
        };
        let mut instance = module.create(id).unwrap();
        let mut processor = instance.activate(48_000.0, 128).unwrap();
        let baseline = instance.layout().clone();
        assert_eq!(baseline.audio_outputs.len(), 1);
        assert!(instance.refresh_capabilities().is_err());
        for phase in 1..=6 {
            eprintln!("dynamic fixture {format} phase {phase}: processing current admission");
            assert!(processor.set_param(0, 12000, phase as f64));
            let mut left = [1.0; 128];
            let mut right = [1.0; 128];
            assert_ne!(
                processor.process(&mut left, &mut right),
                ProcessStatus::Failed
            );
            eprintln!("dynamic process returned; owner idle begins");
            let mut requested = false;
            instance.idle(&mut |notification| {
                requested |= matches!(
                    notification,
                    PluginNotification::RestartRequested
                        | PluginNotification::ParamsRescanned
                        | PluginNotification::LatencyChanged { .. }
                )
            });
            // CLAP executes request_callback first and consumes raised flags on
            // its following owner turn. No sleep or audio work is needed.
            instance.idle(&mut |notification| {
                requested |= matches!(
                    notification,
                    PluginNotification::RestartRequested
                        | PluginNotification::ParamsRescanned
                        | PluginNotification::LatencyChanged { .. }
                )
            });
            assert!(requested, "{format} phase {phase}");
            instance.deactivate(processor).unwrap();
            eprintln!("dynamic fixture {format} phase {phase}: inactive refresh");
            instance.refresh_capabilities().unwrap();
            let state = instance.save_state().unwrap();
            processor = instance.activate(48_000.0, 128).unwrap();
            assert_eq!(processor.latency_samples(), if phase == 1 { 37 } else { 0 });
            assert_eq!(
                instance.layout().audio_inputs.len(),
                if phase == 2 { 2 } else { 1 }
            );
            assert_eq!(
                instance.layout().audio_outputs.len(),
                if phase == 2 { 3 } else { 1 }
            );
            assert_eq!(
                instance.layout().note_inputs,
                if phase == 4 { 2 } else { 1 }
            );
            assert_eq!(instance.param(31).is_some(), phase != 3);
            assert_eq!(instance.param(32).is_some(), phase == 3);
            if format == "clap" {
                assert_eq!(processor.supports_note_instances(), phase != 5);
            } else {
                assert_eq!(processor.note_expression_mask(0) != 0, phase == 5);
            }
            left.fill(0.0);
            right.fill(0.0);
            left[0] = 1.0;
            right[0] = 1.0;
            processor.process(&mut left, &mut right);
            if phase == 1 {
                assert!(left[..37].iter().all(|value| *value == 0.0));
                assert!(left[37] > 0.0);
                assert!(left[38..].iter().all(|value| *value == 0.0));
            }
            if phase == 2 {
                let mut mono = [[0.0; 2]; 128];
                let mut stereo = mono;
                assert!(processor.copy_output(1, &mut mono));
                assert!(processor.copy_output(2, &mut stereo));
                assert_eq!(mono, [[0.11, 0.11]; 128]);
                assert_eq!(stereo, [[0.21, 0.22]; 128]);
            }
            let mut reopened = module.create(id).unwrap();
            eprintln!("dynamic fixture {format} phase {phase}: reopen state");
            reopened
                .load_state(&PluginState::from_bytes(state.into_bytes()))
                .unwrap();
            assert_eq!(reopened.layout(), instance.layout());
            assert_eq!(
                reopened.params().iter().map(|p| p.id).collect::<Vec<_>>(),
                instance.params().iter().map(|p| p.id).collect::<Vec<_>>()
            );
        }
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn a_note_offer_the_plugin_changes_itself_asks_for_a_restart_and_waits_for_it() {
    use windfall_plugin_host::NoteExpressionKind as K;
    // The CLAP switching sine's note dialect (parameter 2) and what VST3
    // class 29 withholds per note (parameter 8 pan, 9 volume) take effect
    // at the next activation, and a running plugin asks for that itself.
    let both = K::Pan.mask() | K::Volume.mask();
    for format in ["clap", "vst3"] {
        let path = common::plugin_file("self-restarting", &format!("switch.{format}"));
        let module = PluginHost::windfall().load(&path).unwrap();
        let id = if format == "clap" {
            "org.windfall.test.switching-sine".to_owned()
        } else {
            module.descriptors()[29].id.clone()
        };
        let mut instance = module.create(&id).unwrap();
        // What it offers per note on the first channel: note instances for
        // CLAP, the pan and volume expressions for VST3.
        let offered = |processor: &windfall_plugin_host::PluginProcessor| {
            if format == "clap" {
                u32::from(processor.supports_note_instances())
            } else {
                processor.note_expression_mask(0) & both
            }
        };
        let steps: Vec<(Vec<(u32, f64)>, u32)> = if format == "clap" {
            vec![(vec![(2, 1.0)], 0), (vec![(2, 0.0)], 1)]
        } else {
            vec![
                (vec![(8, 1.0)], K::Volume.mask()),
                (vec![(9, 1.0)], 0),
                (vec![(8, 0.0), (9, 0.0)], both),
            ]
        };
        let mut processor = instance.activate(48_000.0, 128).unwrap();
        processor.set_realtime(false);
        let mut before = offered(&processor);
        assert_eq!(before, if format == "clap" { 1 } else { both }, "{format}");
        for (switch, after) in steps {
            for &(param, value) in &switch {
                assert!(processor.set_param(0, param, value));
            }
            let (mut left, mut right) = ([0.0; 128], [0.0; 128]);
            assert_ne!(
                processor.process(&mut left, &mut right),
                ProcessStatus::Failed
            );
            // The processed value reaches a VST3 controller on the owner's
            // turn; a CLAP plugin raises the request from its process call.
            let mut requested = false;
            for _ in 0..2 {
                instance.idle(&mut |notification| {
                    requested |= matches!(notification, PluginNotification::RestartRequested)
                });
            }
            assert!(
                requested,
                "{format} {switch:?}: the plugin asked to be restarted"
            );
            assert_eq!(
                offered(&processor),
                before,
                "{format} {switch:?}: the running plugin keeps its offer"
            );
            instance.deactivate(processor).unwrap();
            instance.refresh_capabilities().unwrap();
            processor = instance.activate(48_000.0, 128).unwrap();
            processor.set_realtime(false);
            assert_eq!(offered(&processor), after, "{format} {switch:?}");
            let mut again = false;
            instance.idle(&mut |notification| {
                again |= matches!(notification, PluginNotification::RestartRequested)
            });
            assert!(!again, "{format} {switch:?}: nothing more to announce");
            before = after;
        }
        instance.deactivate(processor).unwrap();
    }
}

#[test]
fn native_extra_buses_follow_fixed_and_threaded_pipeline_delays() {
    use windfall_plugin_host::wrapper::{ProcessingMode, WrapperOptions};
    for format in ["clap", "vst3"] {
        let path = common::plugin_file("dynamic-wrapper-outputs", &format!("fixture.{format}"));
        let module = PluginHost::windfall().load(&path).unwrap();
        let id = if format == "clap" {
            "org.windfall.test.dynamic"
        } else {
            "0000000000000000000000000000001A"
        };
        for (processing, fixed_block, latency) in [
            (ProcessingMode::InProcess, Some(128), 128),
            (ProcessingMode::Threaded, None, 256),
        ] {
            let mut instance = module.create(id).unwrap();
            assert!(instance.set_param(12000, 2.0));
            instance.refresh_capabilities().unwrap();
            let mut processor = instance
                .activate_with_options(
                    48_000.0,
                    128,
                    &WrapperOptions {
                        processing,
                        fixed_block,
                        ..Default::default()
                    },
                )
                .unwrap();
            processor.set_realtime(false);
            assert_eq!(processor.latency_samples(), latency as u32);
            let mut measured = Vec::new();
            for _ in 0..8 {
                assert_ne!(
                    processor.process(&mut [0.0; 128], &mut [0.0; 128]),
                    ProcessStatus::Failed
                );
                let mut output = [[0.0; 2]; 128];
                assert!(processor.copy_output(2, &mut output));
                measured.extend(output);
            }
            assert_eq!(
                measured.iter().position(|frame| frame[0] != 0.0),
                Some(latency)
            );
            assert!(measured[..latency].iter().all(|frame| *frame == [0.0; 2]));
            assert!(
                measured[latency..]
                    .iter()
                    .all(|frame| *frame == [0.21, 0.22])
            );
            instance.deactivate(processor).unwrap();
        }
    }
}
