//! Native fixture-backed wrapper contracts, authored but not executed here.
mod common;
use windfall_plugin_host::{
    PluginHost,
    wrapper::{ProcessingMode, WrapperOptions},
};
#[test]
fn threaded_native_vst3_deactivation_refusal_returns_same_worker_pipeline() {
    let file = common::plugin_file("threaded-refusal", "threaded.vst3");
    let module = PluginHost::windfall().load(&file).unwrap();
    let mut instance = module.create(&module.descriptors()[6].id).unwrap();
    let options = WrapperOptions {
        processing: ProcessingMode::Threaded,
        ..Default::default()
    };
    let processor = instance
        .activate_with_options(48_000.0, 64, &options)
        .unwrap();
    let refused = instance.deactivate(processor).unwrap_err();
    assert!(instance.is_active());
    // State is read while the plugin stays active, so a save while music
    // plays never needs the deactivation this plugin refuses.
    assert!(instance.save_state().is_ok());
    let mut processor = refused.returned;
    processor.set_realtime(false);
    assert_eq!(processor.latency_samples(), 145);
    assert!(processor.set_param(0, 7, 0.75));
    for _ in 0..5 {
        assert_ne!(
            processor.process(&mut [1.0; 64], &mut [1.0; 64]),
            windfall_plugin_host::ProcessStatus::Failed
        );
    }
    let refused = instance.deactivate(processor).unwrap_err();
    assert!(
        refused
            .error
            .to_string()
            .contains("setProcessing(false) refused")
    );
    let mut processor = refused.returned;
    assert!(processor.set_param(0, 7, 0.25));
    let mut left = [1.0; 64];
    let mut right = left;
    for _ in 0..5 {
        left.fill(1.0);
        right.fill(1.0);
        assert_ne!(
            processor.process(&mut left, &mut right),
            windfall_plugin_host::ProcessStatus::Failed
        );
    }
    assert_eq!(left, [0.5; 64]);
    assert_eq!(right, left);
    instance.deactivate(processor).unwrap();
    assert!(!instance.is_active());
    assert_eq!(instance.param_value(7), Some(0.25));
}
#[test]
fn threaded_native_clap_and_vst3_capture_reopen_preserves_latest_collected_control() {
    for vst3 in [false, true] {
        let file = common::plugin_file(
            "threaded-native",
            if vst3 {
                "threaded.vst3"
            } else {
                "threaded.clap"
            },
        );
        let module = PluginHost::windfall().load(&file).unwrap();
        let id = if vst3 {
            module.descriptors()[0].id.clone()
        } else {
            common::GAIN.to_owned()
        };
        let mut instance = module.create(&id).unwrap();
        let options = WrapperOptions {
            processing: ProcessingMode::Threaded,
            ..Default::default()
        };
        let mut processor = instance
            .activate_with_options(48_000.0, 64, &options)
            .unwrap();
        processor.set_realtime(false);
        assert_eq!(processor.latency_samples(), instance.latency_samples());
        let native_latency = if vst3 { 17 } else { 0 };
        assert_eq!(processor.latency_samples(), 128 + native_latency);
        let value = if vst3 { 0.25 } else { 0.5 };
        assert!(processor.set_param(0, 7, value));
        let mut actual = Vec::new();
        for _ in 0..5 {
            let mut left = [1.0; 64];
            let mut right = left;
            assert_ne!(
                processor.process(&mut left, &mut right),
                windfall_plugin_host::ProcessStatus::Failed
            );
            actual.extend(left);
        }
        assert_eq!(&actual[..128], &[0.0; 128]);
        assert_eq!(&actual[128..], &[0.5; 192]);
        assert!(processor.set_param(0, 7, 0.75));
        processor.process(&mut [0.0; 1], &mut [0.0; 1]);
        assert!(
            !processor.controls_processed(),
            "collection is not native DSP completion"
        );
        instance.deactivate(processor).unwrap();
        assert_eq!(instance.param_value(7), Some(0.75));
        let state = instance.save_state().unwrap();
        let mut reopened = module.create(&id).unwrap();
        reopened.load_state(&state).unwrap();
        assert_eq!(reopened.param_value(7), Some(0.75));
    }
}
#[test]
fn fixed_native_buffers_delay_pcm_exactly_and_preserve_pending_state_on_return() {
    let host = PluginHost::windfall();
    let module = host
        .load(&common::plugin_file("wrapper", "wrapper.clap"))
        .unwrap();
    let mut instance = module.create(common::GAIN).unwrap();
    let options = WrapperOptions {
        fixed_block: Some(64),
        ..Default::default()
    };
    let mut processor = instance
        .activate_with_options(48_000.0, 128, &options)
        .unwrap();
    assert_eq!(processor.latency_samples(), 64);
    assert_eq!(instance.latency_samples(), 64);
    assert!(processor.set_param(0, 7, 0.5));
    let mut actual = Vec::new();
    let mut position = 0;
    for frames in [1, 7, 31, 64, 3, 128, 22] {
        let mut left: Vec<_> = (position..position + frames)
            .map(|i| i as f32 / 1024.0)
            .collect();
        let mut right = left.clone();
        processor.process(&mut left, &mut right);
        actual.extend(left);
        position += frames;
    }
    for (frame, value) in actual.into_iter().enumerate() {
        let expected = if frame < 64 {
            0.0
        } else {
            (frame - 64) as f32 / 2048.0
        };
        assert!((value - expected).abs() < 1e-6);
    }
    assert!(processor.set_param(0, 7, 0.75));
    processor.process(&mut [0.0; 1], &mut [0.0; 1]);
    instance.deactivate(processor).unwrap();
    assert_eq!(instance.param_value(7), Some(0.75));
}
#[test]
fn wrapper_legacy_defaults_and_unsupported_owner_modes_are_explicit() {
    let defaults: WrapperOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults, WrapperOptions::default());
    {
        let processing = ProcessingMode::Bridged;
        let (_module, mut instance) = common::create(common::GAIN);
        assert!(
            instance
                .activate_with_options(
                    48_000.0,
                    64,
                    &WrapperOptions {
                        processing,
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert!(!instance.is_active());
    }
    assert!(
        WrapperOptions {
            fixed_block: Some(65),
            ..Default::default()
        }
        .validate(64)
        .is_err()
    );
}
