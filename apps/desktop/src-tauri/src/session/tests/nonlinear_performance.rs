//! Buffered effect MIDI acceptance through the production controller link pump.
//! Synthetic input messages, no port or audio device; written, not executed.
use std::time::Instant;

use super::{Rig, rms};
use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
use windfall_project::controller_links::PickupMode;
use windfall_project::{
    AutomationTarget, ChannelId, Command, ControlMap, ControlSource, ControllerLink, EffectId,
    EffectKind, InstrumentKind, LinkTarget, MidiControl, TrackId,
};

fn message(rig: &Rig, key: u8, down: bool, channel: u8) -> bool {
    rig.session.controller_message(InputMessage {
        device: 0,
        message: if down {
            MidiMessage::NoteOn {
                channel,
                key,
                velocity: 100,
            }
        } else {
            MidiMessage::NoteOff { channel, key }
        },
        at: Instant::now(),
        stamp_micros: 0,
    })
}

fn pump(rig: &Rig) {
    rig.session.controllers_pump(Instant::now());
}

fn trigger(rig: &Rig, effect: EffectId, index: usize) -> f32 {
    rig.project()
        .mixer
        .tracks
        .iter()
        .flat_map(|t| &t.effects)
        .find(|e| e.id == effect)
        .unwrap()
        .params
        .get(index)
        .unwrap()
}

#[test]
fn midi_notes_capture_and_release_both_buffered_processors_through_saved_controller_links() {
    for kind in [EffectKind::VolumeGate, EffectKind::TimeTransport] {
        let mut rig = Rig::new();
        let added = rig
            .session
            .dispatch(
                Command::AddChannel {
                    name: Some("Capture fixture".into()),
                    sample: None,
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap();
        let channel = ChannelId(added.created[0]);
        let track = TrackId(added.created[1]);
        let effect = EffectId(
            rig.session
                .dispatch(
                    Command::AddEffect {
                        track,
                        kind,
                        index: None,
                    },
                    None,
                )
                .unwrap()
                .created[0],
        );
        let descriptors = kind.descriptors();
        let trigger_index = descriptors.iter().position(|d| d.id == "trigger").unwrap();
        let loop_index = descriptors
            .iter()
            .position(|d| d.id == "loopBeats")
            .unwrap();
        rig.session
            .dispatch(
                Command::SetEffectParam {
                    track,
                    effect,
                    param: loop_index as u32,
                    value: 0.125,
                },
                None,
            )
            .unwrap();
        let link = ControllerLink {
            source: ControlSource::Midi {
                device: None,
                channel: Some(1),
                control: MidiControl::Note { key: 36 },
            },
            target: LinkTarget::Param {
                target: AutomationTarget::EffectParam {
                    track,
                    effect,
                    param: trigger_index as u32,
                },
            },
            map: ControlMap {
                pickup: PickupMode::Off,
                ..Default::default()
            },
        };
        rig.session
            .dispatch(
                Command::SetControllerLinks {
                    links: vec![link.clone()],
                },
                None,
            )
            .unwrap();
        pump(&rig);
        assert!(
            !message(&rig, 37, true, 0),
            "unmapped keys remain available for audition"
        );
        assert!(
            !message(&rig, 36, true, 1),
            "the link filters MIDI channels"
        );
        rig.session.audition_note_on(channel, 69, 0.7);
        assert!(rms(&rig.run(12_000)) > 0.01);

        assert!(
            message(&rig, 36, true, 0),
            "linked notes are consumed before audition"
        );
        pump(&rig);
        assert_eq!(trigger(&rig, effect, trigger_index), 1.0);
        rig.run(480);
        rig.session.audition_note_off(channel, 69);
        // The live synth dies away, but the immutable pre-release slice remains.
        rig.run(48_000);
        assert!(
            rms(&rig.run(6000)) > 0.01,
            "{kind:?} held capture remains audible"
        );
        let revision = rig.session.document_snapshot().revision;
        assert!(message(&rig, 36, true, 0));
        pump(&rig);
        assert_eq!(
            rig.session.document_snapshot().revision,
            revision,
            "duplicate down is a no-op"
        );
        assert!(message(&rig, 36, false, 0));
        pump(&rig);
        assert_eq!(trigger(&rig, effect, trigger_index), 0.0);
        rig.run(4800);
        assert!(
            rms(&rig.run(6000)) < 1e-5,
            "{kind:?} release resumes silent live input"
        );
        assert_eq!(rig.session.controller_state().error, None);

        let saved = rig.project();
        let path = rig
            .session
            .project_save(Some(&rig.file("buffered-midi")))
            .unwrap();
        let opened = rig.session.project_open(&path).unwrap();
        assert_eq!(opened.project, saved);
        assert_eq!(opened.project.settings.controller_links, vec![link]);
        pump(&rig);
        assert!(
            message(&rig, 36, true, 0),
            "reopened link still consumes the key"
        );
        pump(&rig);
        assert_eq!(trigger(&rig, effect, trigger_index), 1.0);
        rig.session
            .dispatch(Command::RemoveEffect { track, effect }, None)
            .unwrap();
        pump(&rig);
        assert!(rig.project().settings.controller_links.is_empty());
        assert!(
            !message(&rig, 36, false, 0),
            "removed target leaves no stuck key mapping"
        );
        rig.session.undo().unwrap();
        pump(&rig);
        assert!(message(&rig, 36, false, 0));
        pump(&rig);
        assert_eq!(trigger(&rig, effect, trigger_index), 0.0);
        rig.session.project_new().unwrap();
        pump(&rig);
        assert!(
            !message(&rig, 36, true, 0),
            "project replacement discards old mappings"
        );
    }
}
