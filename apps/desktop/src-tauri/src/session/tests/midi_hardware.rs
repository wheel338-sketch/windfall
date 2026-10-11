use super::Rig;
use crate::session::recording::{CaptureHandle, Sink};
use windfall_ipc::{MidiHardwareSettings, RecordingSource};
use windfall_project::{ChannelId, Command};

fn select(rig: &Rig, channel: ChannelId) {
    let generation = rig.session.midi_hardware_state().generation;
    let revision = rig.session.document_snapshot().revision;
    rig.session
        .midi_hardware_target(Some(channel), generation, revision)
        .unwrap();
}

#[test]
fn midi_hardware_audition_does_not_edit_history_and_stop_discards_pending_notes() {
    let mut rig = Rig::new();
    let channel = rig.channel(0);
    let before = rig.session.document_snapshot();
    select(&rig, channel);
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 60, 100));
    assert!(rig.run(512).iter().any(|sample| *sample != 0.0));
    assert!(rig.session.hardware_note(epoch, 60, 0));
    assert_eq!(rig.session.document_snapshot(), before);
    rig.session.transport_stop();
    assert!(!rig.session.hardware_note(epoch, 60, 100));
    rig.run(2048);
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn midi_hardware_new_project_removed_channel_and_stale_target_requests_are_discarded() {
    let mut rig = Rig::new();
    let channel = rig.channel(0);
    let before = rig.session.document_snapshot();
    let generation = rig.session.midi_hardware_state().generation;
    select(&rig, channel);
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 60, 100));
    rig.session.project_new().unwrap();
    assert_eq!(rig.session.midi_hardware_state().target, None);
    assert!(!rig.session.hardware_note(epoch, 60, 100));
    // The default project reuses channel IDs and revision zero. Generation protects it.
    assert_eq!(rig.channel(0), channel);
    assert_eq!(rig.session.document_snapshot().revision, before.revision);
    assert!(
        rig.session
            .midi_hardware_target(Some(channel), generation, before.revision)
            .is_err()
    );
    rig.run(2048);
    assert_eq!(rig.session.controller().frame().voices, 0);
    select(&rig, channel);
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 60, 100));
    rig.run(512);
    rig.session
        .dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert_eq!(rig.session.midi_hardware_state().target, None);
    assert!(!rig.session.hardware_note(epoch, 60, 100));
    rig.run(2048);
    assert_eq!(rig.session.controller().frame().voices, 0);
    rig.session.undo().unwrap();
    assert_eq!(rig.session.midi_hardware_state().target, None);
}

#[test]
fn midi_hardware_builtin_synth_note_off_and_panic_release_without_project_edits() {
    let mut rig = Rig::new();
    let result = rig
        .session
        .dispatch(
            Command::AddChannel {
                name: Some("MIDI synth".into()),
                sample: None,
                instrument: Some(windfall_project::InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    select(&rig, ChannelId(result.created[0]));
    let before = rig.session.document_snapshot();
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 69, 127));
    assert!(rig.run(2048).iter().any(|sample| sample.abs() > 0.01));
    assert!(rig.session.hardware_note(epoch, 69, 0));
    rig.run(48_000);
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert!(rig.session.hardware_note(epoch, 60, 100));
    rig.run(512);
    rig.session.midi_hardware_panic();
    rig.run(2048);
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert_eq!(rig.session.document_snapshot(), before);
}

struct Capture;
impl CaptureHandle for Capture {
    fn frames(&self) -> u64 {
        480
    }
    fn failed(&self) -> bool {
        false
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Ok(480)
    }
}
fn capture(_: RecordingSource, _: u32, mut sink: Sink) -> Result<Box<dyn CaptureHandle>, String> {
    sink(&[0.25; 960])?;
    Ok(Box::new(Capture))
}

#[test]
fn midi_hardware_device_and_target_changes_respect_recording_exclusion() {
    let rig = Rig::new();
    select(&rig, rig.channel(0));
    rig.session
        .recording_start_with(
            RecordingSource {
                host: "Fake".into(),
                device: "Fake".into(),
                left: 0,
                right: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
                armed_tracks: None,
                mixer_tap: None,
            },
            960,
            None,
            capture,
        )
        .unwrap();
    let take = rig.session.recording_state();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .midi_hardware_configure(MidiHardwareSettings::default())
            .unwrap_err()
            .contains("recording")
    );
    assert!(
        rig.session
            .midi_hardware_target(None, 0, before.revision)
            .unwrap_err()
            .contains("recording")
    );
    rig.session.midi_hardware_panic();
    assert_eq!(rig.session.recording_state(), take);
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        rig.session.midi_hardware_state().target,
        Some(rig.channel(0))
    );
    rig.session.recording_cancel();
    rig.session
        .midi_hardware_target(None, 0, before.revision)
        .unwrap();
}

#[test]
fn midi_hardware_settings_persist_without_reusing_the_runtime_destination() {
    use std::sync::Arc;
    use windfall_engine::midi_hardware::{Audition, Ingress, Output, Ports, Runtime};
    struct SilentPorts;
    impl Ports for SilentPorts {
        fn enumerate(
            &mut self,
        ) -> Result<(Vec<windfall_ipc::MidiPort>, Vec<windfall_ipc::MidiPort>), String> {
            Ok((vec![], vec![]))
        }
        fn input(&mut self, _: &str, _: Ingress) -> Result<Box<dyn Send>, String> {
            Err("Fake port unavailable".into())
        }
        fn output(&mut self, _: &str) -> Result<Box<dyn Output>, String> {
            Err("Fake port unavailable".into())
        }
    }
    struct SilentTarget;
    impl Audition for SilentTarget {
        fn note(&self, _: u64, _: u8, _: u8) -> bool {
            false
        }
    }
    let rig = Rig::new();
    let runtime = Runtime::start_with(
        rig.session.controller().clone(),
        Arc::new(SilentTarget),
        MidiHardwareSettings::default(),
        || Box::new(SilentPorts),
    )
    .unwrap();
    *crate::sync::lock(&rig.session.inner.midi_hardware) = Some(Arc::new(runtime));
    select(&rig, rig.channel(0));
    let before = rig.session.document_snapshot();
    let settings = MidiHardwareSettings {
        input: Some("saved-input".into()),
        output: Some("saved-output".into()),
        input_channel: Some(4),
        output_channel: 7,
        ..MidiHardwareSettings::default()
    };
    let result = rig
        .session
        .midi_hardware_configure(settings.clone())
        .unwrap();
    assert!(!result.input_connected && !result.output_connected && result.error.is_some());
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(
        rig.session
            .midi_hardware_configure(MidiHardwareSettings {
                output_channel: 0,
                ..settings.clone()
            })
            .is_err()
    );
    let rig = rig.restart();
    assert_eq!(rig.session.midi_hardware_state().settings, settings);
    assert_eq!(rig.session.midi_hardware_state().target, None);
}

#[cfg(windows)]
#[test]
fn midi_hardware_native_clap_fixture_audition_note_off_and_panic_preserve_ownership() {
    use windfall_engine::plugins::PluginFactory;
    use windfall_project::PluginTarget;
    let mut rig = Rig::new();
    let (manager, path) = super::plugins::manager(&rig);
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.sine",
            PluginTarget::Instrument {
                channel: ChannelId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    select(&rig, ChannelId(added.created[0]));
    let before = rig.session.document_snapshot();
    let revision = manager.runtime.revision();
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 60, 100));
    assert!(rig.run(2048).iter().any(|sample| sample.abs() > 0.01));
    assert!(rig.session.hardware_note(epoch, 60, 0));
    rig.run(48_000);
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert!(rig.session.hardware_note(epoch, 64, 100));
    rig.run(512);
    rig.session.midi_hardware_panic();
    rig.run(2048);
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert_eq!(manager.runtime.revision(), revision);
    assert_eq!(rig.session.document_snapshot(), before);
}

#[cfg(windows)]
#[test]
fn midi_hardware_native_clap_saturated_panic_releases_queued_notes() {
    use windfall_engine::plugins::PluginFactory;
    use windfall_project::PluginTarget;
    let mut rig = Rig::new();
    let (manager, path) = super::plugins::manager(&rig);
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.sine",
            PluginTarget::Instrument {
                channel: ChannelId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    select(&rig, ChannelId(added.created[0]));
    rig.run(2048); // Install and warm the native adapter before saturating it.
    let before = rig.session.document_snapshot();
    let revision = manager.runtime.revision();
    let epoch = rig.session.controller().hardware_epoch();
    for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
        assert!(rig.session.hardware_note(epoch, 64, 100));
    }
    // Handle engine messages without rendering: all note-ons remain in the
    // native processor's bounded event queue when the panic arrives.
    rig.processor.process(&mut []);
    rig.session.midi_hardware_panic();
    let out = rig.run(4096);
    assert!(
        out.iter().all(|sample| *sample == 0.0),
        "queued hardware notes sounded after panic"
    );
    assert_eq!(rig.session.controller().frame().voices, 0);
    rig.session.midi_hardware_panic();
    assert!(rig.run(2048).iter().all(|sample| *sample == 0.0));
    assert_eq!(manager.runtime.revision(), revision);
    assert_eq!(rig.session.document_snapshot(), before);
}

#[cfg(windows)]
#[test]
fn midi_hardware_native_clap_saturated_panic_preserves_a_ui_key() {
    use windfall_project::PluginTarget;
    let mut rig = Rig::new();
    let (manager, path) = super::plugins::manager(&rig);
    let binding = manager
        .binding(
            &path,
            "org.windfall.test.sine",
            PluginTarget::Instrument {
                channel: ChannelId(0),
            },
        )
        .unwrap();
    let added = rig
        .session
        .dispatch(Command::AddPluginInstrument { plugin: binding }, None)
        .unwrap();
    let channel = ChannelId(added.created[0]);
    select(&rig, channel);
    rig.run(2048);
    let before = rig.session.document_snapshot();
    let epoch = rig.session.controller().hardware_epoch();
    // Two hardware keys ensure that the fixture's first-voice stealing under
    // saturation leaves a hardware voice to release as well as UI voices.
    assert!(rig.session.hardware_note(epoch, 64, 100));
    assert!(rig.session.hardware_note(epoch, 67, 100));
    rig.session.audition_note_on(channel, 69, 0.8);
    rig.run(2048);
    for _ in 0..windfall_plugin_host::EVENT_CAPACITY {
        rig.session.audition_note_on(channel, 69, 0.8);
    }
    rig.processor.process(&mut []);
    rig.session.midi_hardware_panic();
    assert!(
        rig.run(4096).iter().any(|sample| sample.abs() > 0.01),
        "independent UI key must survive hardware panic"
    );
    assert_eq!(rig.session.controller().frame().voices, 1);
    rig.session.midi_hardware_panic();
    assert!(rig.run(2048).iter().any(|sample| sample.abs() > 0.01));
    rig.session.audition_note_off(channel, 69);
    rig.run(2048);
    assert!(
        rig.run(2048).iter().all(|sample| *sample == 0.0),
        "hardware key survived panic after UI release"
    );
    assert_eq!(rig.session.controller().frame().voices, 0);
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn midi_hardware_notes_are_reported_to_the_ui_for_the_played_note_log() {
    use crate::events::Event;
    let rig = Rig::new();
    let channel = rig.channel(0);
    select(&rig, channel);
    rig.events.take();
    let epoch = rig.session.controller().hardware_epoch();
    assert!(rig.session.hardware_note(epoch, 60, 100));
    assert!(rig.session.hardware_note(epoch, 60, 0));
    let reported: Vec<Event> = rig
        .events
        .take()
        .into_iter()
        .filter(|event| matches!(event, Event::MidiHardwareNote(_)))
        .collect();
    let notes: Vec<_> = reported
        .iter()
        .map(|event| match event {
            Event::MidiHardwareNote(note) => (note.channel, note.key, note.velocity),
            _ => unreachable!("filtered above"),
        })
        .collect();
    // A press and its release, in order, for the channel that sounded them.
    assert_eq!(notes, [(channel, 60, 100), (channel, 60, 0)]);
    assert_eq!(reported[0].name(), "midi:note");
    assert_eq!(
        reported[0].payload().unwrap(),
        format!(r#"{{"channel":{},"key":60,"velocity":100}}"#, channel.0)
    );
    // A note the engine refuses is not reported, so the log never holds a
    // note that did not sound.
    rig.session.transport_stop();
    assert!(!rig.session.hardware_note(epoch, 60, 100));
    assert!(
        rig.events
            .take()
            .iter()
            .all(|event| !matches!(event, Event::MidiHardwareNote(_)))
    );
}
