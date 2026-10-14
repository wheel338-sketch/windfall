//! What the host does about plugins that misbehave.

mod common;

use std::time::Duration;

use common::{
    ABSURD_PORTS, GAIN, INIT_FAIL, NAN, PROCESS_ERROR, PROCESS_PANIC, SINE, SLOW, check_program,
    create, signal, test_plugins,
};
use windfall_plugin_host::{CheckStep, PluginError, ProcessStatus, StepOutcome, check_plugin};

const RATE: f64 = 48_000.0;

#[test]
fn output_that_is_not_a_number_never_leaves_the_plugin() {
    let (_module, mut instance) = create(NAN);
    let mut processor = instance.activate(RATE, 256).unwrap();
    let (mut left, mut right) = signal(256);
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert!(left.iter().chain(&right).all(|&sample| sample == 0.0));

    let health = processor.health();
    assert_eq!(health.scrubbed_samples, 512);
    assert!(!health.failed);
    assert_eq!(instance.health(), health, "the main thread reads the same");
    instance.deactivate(processor).unwrap();
}

#[test]
fn a_plugin_that_reports_a_failure_is_not_called_again() {
    let (_module, mut instance) = create(PROCESS_ERROR);
    let mut processor = instance.activate(RATE, 256).unwrap();
    for block in 0..6 {
        let (mut left, mut right) = signal(256);
        let (dry_left, dry_right) = (left.clone(), right.clone());
        let status = processor.process(&mut left, &mut right);
        // The plugin fails from its third block on.
        assert_eq!(status == ProcessStatus::Failed, block >= 2, "block {block}");
        if block == 2 {
            // The block it failed in was processed in place, so what it
            // left behind is replaced by silence.
            assert!(left.iter().chain(&right).all(|&sample| sample == 0.0));
        } else {
            // Before, the plugin passes its input through. After, the host
            // does.
            assert_eq!(left, dry_left);
            assert_eq!(right, dry_right);
        }
    }
    assert!(processor.health().failed);
    assert!(instance.health().failed);
    instance.deactivate(processor).unwrap();

    // Activating again gives the plugin another chance.
    let mut processor = instance.activate(RATE, 256).unwrap();
    let (mut left, mut right) = signal(256);
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    instance.deactivate(processor).unwrap();
}

#[test]
fn a_slow_block_is_counted_on_a_live_device_and_not_in_a_render() {
    let (_module, mut instance) = create(SLOW);
    let mut processor = instance.activate(RATE, 256).unwrap();
    // 256 frames last 5.3 ms. The plugin takes 30 ms for them.
    let (mut left, mut right) = signal(256);
    processor.process(&mut left, &mut right);
    let health = processor.health();
    assert_eq!(health.overruns, 1);
    assert!(health.worst_overrun_micros > 15_000, "{health:?}");

    processor.set_realtime(false);
    processor.process(&mut left, &mut right);
    assert_eq!(processor.health().overruns, 1);
    instance.deactivate(processor).unwrap();

    // A plugin that keeps up sets nothing. The blocks are long, so that a
    // busy test machine cannot make one late by pausing the thread.
    let (_fast_module, mut fast) = create(GAIN);
    let mut processor = fast.activate(RATE, 8_192).unwrap();
    let (mut left, mut right) = signal(8_192);
    for _ in 0..20 {
        processor.process(&mut left, &mut right);
    }
    assert_eq!(processor.health().overruns, 0);
    fast.deactivate(processor).unwrap();
}

#[test]
fn a_plugin_with_absurd_ports_is_refused() {
    let (_, module) = common::load();
    let Err(error) = module.create(ABSURD_PORTS) else {
        panic!("a plugin with four billion ports was created");
    };
    let PluginError::Layout(reason) = error else {
        panic!("not a layout refusal: {error:?}");
    };
    assert_eq!(
        windfall_core::english::english(&reason),
        "it reports 4294967295 audio inputs"
    );
}

#[test]
fn a_plugin_whose_init_fails_is_an_error() {
    let (_, module) = common::load();
    assert!(matches!(
        module.create(INIT_FAIL),
        Err(PluginError::Create(_))
    ));
}

#[test]
fn a_file_that_is_not_a_plugin_is_an_error() {
    let host = windfall_plugin_host::PluginHost::windfall();
    let folder = common::scratch().join("not-plugins");
    std::fs::create_dir_all(&folder).unwrap();
    let text = folder.join("notes.txt");
    std::fs::write(&text, b"hello").unwrap();
    assert!(matches!(host.load(&text), Err(PluginError::Load(_))));
    let fake = folder.join("fake.clap");
    std::fs::write(&fake, b"this is not a library").unwrap();
    assert!(matches!(host.load(&fake), Err(PluginError::Load(_))));
    assert!(matches!(
        host.load(&folder.join("missing.clap")),
        Err(PluginError::Load(_))
    ));
}

#[test]
fn a_healthy_plugin_passes_every_stage_of_a_check() {
    for id in [GAIN, SINE] {
        let report = check_plugin(
            check_program(),
            &test_plugins(),
            id,
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(report.all_passed(), "{id}: {report:?}");
    }
    let report = check_plugin(
        check_program(),
        &test_plugins(),
        SINE,
        Duration::from_secs(20),
    )
    .unwrap();
    let StepOutcome::Passed(detail) = report.outcome(CheckStep::Process) else {
        panic!("{report:?}");
    };
    // Velocity 0.8 at the default level of 0.5.
    assert!(detail.contains("output peak 0.4000"), "{detail}");
}

#[test]
fn a_plugin_that_panics_while_processing_only_ends_the_check_process() {
    // A panic cannot cross from the plugin into the host: Rust ends the
    // process instead. In a process of its own that costs nothing.
    let report = check_plugin(
        check_program(),
        &test_plugins(),
        PROCESS_PANIC,
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(report.outcome(CheckStep::Activate).passed(), "{report:?}");
    assert!(
        matches!(report.outcome(CheckStep::Process), StepOutcome::Crashed(_)),
        "{report:?}"
    );
    assert_eq!(*report.outcome(CheckStep::State), StepOutcome::NotReached);
}

#[test]
fn a_check_reports_a_plugin_that_fails_in_the_middle() {
    let report = check_plugin(
        check_program(),
        &test_plugins(),
        PROCESS_ERROR,
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(
        *report.outcome(CheckStep::Process),
        StepOutcome::Failed("the plugin reported an error while processing".to_owned())
    );
    assert_eq!(*report.outcome(CheckStep::Params), StepOutcome::NotReached);

    let report = check_plugin(
        check_program(),
        &test_plugins(),
        ABSURD_PORTS,
        Duration::from_secs(20),
    )
    .unwrap();
    assert!(report.outcome(CheckStep::Load).passed());
    assert!(matches!(
        report.outcome(CheckStep::Create),
        StepOutcome::Failed(_)
    ));
}
