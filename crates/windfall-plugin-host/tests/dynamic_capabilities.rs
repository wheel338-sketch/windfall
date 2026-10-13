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
