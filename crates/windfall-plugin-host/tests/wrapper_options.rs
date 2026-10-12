//! Native fixture-backed wrapper contracts, authored but not executed here.
mod common;
use windfall_plugin_host::{
    PluginHost,
    wrapper::{ProcessingMode, WrapperOptions},
};
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
    for processing in [ProcessingMode::Threaded, ProcessingMode::Bridged] {
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
