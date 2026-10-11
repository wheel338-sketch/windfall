//! Instruments and effects through the session: edited with commands,
//! played, saved, reopened and exported.

use std::fs;

use windfall_core::{AudioBuffer, db_to_gain};
use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, PlayMode};
use windfall_project::{
    ChannelId, ChannelSource, Command, EffectId, EffectKind, EffectSlotPatch, InstrumentKind,
    NoteInit, TrackId,
};

use super::{Rig, SAMPLE_RATE, rms};
use crate::events::Event;

/// Frames by which the synth and a limiter at its default look-ahead put
/// their output out late at the engine's sample rate.
const SYNTH_LATENCY: u32 = 12;
const LIMITER_LATENCY: u32 = 240;

/// Adds a channel that plays the synth, on a mixer track of its own, with
/// three notes in the first pattern. The last of them ends on tick 2160,
/// a little over half way through the bar.
fn add_synth(rig: &Rig) -> (ChannelId, TrackId) {
    let added = rig
        .session
        .dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    let (channel, track) = (ChannelId(added.created[0]), TrackId(added.created[1]));
    let note = |start, length, key| NoteInit {
        start,
        length,
        key,
        velocity: None,
        pan: None,
        expression: None,
    };
    rig.session
        .dispatch(
            Command::AddNotes {
                pattern: rig.pattern(),
                channel,
                notes: vec![note(0, 480, 48), note(960, 480, 55), note(1_920, 240, 60)],
            },
            None,
        )
        .unwrap();
    (channel, track)
}

fn add_effect(rig: &Rig, track: TrackId, kind: EffectKind) -> EffectId {
    let command = Command::AddEffect {
        track,
        kind,
        index: None,
    };
    EffectId(rig.session.dispatch(command, None).unwrap().created[0])
}

/// Sets the setting of an effect that has this id among the descriptors of
/// its kind.
fn set(rig: &Rig, track: TrackId, effect: EffectId, kind: EffectKind, id: &str, value: f32) {
    let settings = kind.descriptors();
    let param = settings.iter().position(|info| info.id == id).unwrap();
    let command = Command::SetEffectParam {
        track,
        effect,
        param: param as u32,
        value,
    };
    rig.session.dispatch(command, None).unwrap();
}

/// Exports one pass of the pattern with two seconds of tail and returns the
/// bytes of the file and the audio in it.
fn export(rig: &Rig, name: &str) -> (Vec<u8>, AudioBuffer) {
    rig.events.take();
    let options = ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 2.0,
        auto_tail: false,
        ..ExportOptions::default()
    };
    rig.session.export_audio(options.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    let bytes = fs::read(&options.path).unwrap();
    (bytes, windfall_codec::decode_file(&options.path).unwrap())
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

/// The first frame of interleaved stereo audio louder than a whisper.
fn onset(audio: &AudioBuffer) -> usize {
    let loud = |frame: &[f32]| peak(frame) > 1e-3;
    audio.samples().chunks(2).position(loud).unwrap()
}

#[test]
fn a_synth_through_effects_plays_saves_reopens_and_exports() {
    let mut rig = Rig::new();
    let (_, track) = add_synth(&rig);
    let latency = |rig: &Rig| rig.session.controller().latency_frames();
    assert_eq!(latency(&rig), SYNTH_LATENCY);

    // It plays, and its voices are counted.
    rig.session.transport_play().unwrap();
    let played = rig.run(12_000);
    assert!(rms(&played) > 0.01);
    assert!(rig.session.realtime_tick().voices >= 1);
    rig.session.transport_stop();
    rig.run(4_800);
    assert_eq!(rig.session.realtime_tick().voices, 0);

    // One bar is 96000 frames, and the last note is over well before frame
    // 70000. What sounds from there on is the tail.
    let (_, dry) = export(&rig, "dry.wav");
    assert_eq!(dry.frames(), 96_000 + 2 * 48_000);
    let tail = |audio: &AudioBuffer| rms(&audio.samples()[70_000 * 2..]);
    assert!(tail(&dry) < 1e-5);

    // A reverb on the synth's track rings on after the notes.
    add_effect(&rig, track, EffectKind::Reverb);
    let (_, wet) = export(&rig, "wet.wav");
    assert_eq!(wet.frames(), dry.frames());
    assert!(
        tail(&wet) > 1e-3,
        "the reverb left a tail of {}",
        tail(&wet)
    );
    assert_eq!(onset(&wet), onset(&dry));

    // A limiter on the master holds the peak to its ceiling. It looks 5 ms
    // ahead, which the export takes off its front: the first note is where
    // it was.
    let ceiling = db_to_gain(-24.0);
    assert!(peak(wet.samples()) > 1.5 * ceiling);
    let limiter = add_effect(&rig, TrackId::MASTER, EffectKind::Limiter);
    let kind = EffectKind::Limiter;
    set(&rig, TrackId::MASTER, limiter, kind, "ceilingDb", -24.0);
    assert_eq!(latency(&rig), SYNTH_LATENCY + LIMITER_LATENCY);
    let (bytes, limited) = export(&rig, "limited.wav");
    assert_eq!(limited.frames(), dry.frames());
    assert!(peak(limited.samples()) <= ceiling + 1e-6);
    assert!(peak(limited.samples()) > 0.9 * ceiling);
    assert_eq!(onset(&limited), onset(&dry));
    assert!(tail(&limited) > 1e-4);

    // Played, the limiter's work shows on the realtime feed.
    rig.session.transport_play().unwrap();
    rig.run(12_000);
    let frame = rig.session.realtime_tick();
    let reading = frame.gain_reductions.iter().find(|r| r.effect == limiter);
    assert!(reading.unwrap().db > 3.0);
    rig.session.transport_stop();
    rig.run(4_800);

    // An export is the same file every time.
    assert_eq!(export(&rig, "again.wav").0, bytes);

    // Saved and opened in another run of the app, the project is the same,
    // and so is its export.
    let saved = rig.project();
    let path = rig.session.project_save(Some(&rig.file("song"))).unwrap();
    let mut rig = rig.restart();
    let opened = rig.session.project_open(&path).unwrap();
    assert_eq!(opened.project, saved);
    assert!(
        !rig.events
            .take()
            .iter()
            .any(|event| matches!(event, Event::ProjectWarnings(_)))
    );
    assert_eq!(latency(&rig), SYNTH_LATENCY + LIMITER_LATENCY);
    assert_eq!(export(&rig, "reopened.wav").0, bytes);
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.001);
}

#[test]
fn an_automatic_tail_ends_the_export_when_the_sound_does() {
    let rig = Rig::new();
    let (_, track) = add_synth(&rig);
    add_effect(&rig, track, EffectKind::Reverb);
    let export = |name: &str, auto_tail: bool| {
        rig.events.take();
        let options = ExportOptions {
            path: rig.file(name),
            format: ExportFormat::Wav,
            bit_depth: BitDepth::Float32,
            sample_rate: SAMPLE_RATE,
            mode: PlayMode::Pattern,
            pattern_loops: 1,
            tail_secs: 30.0,
            auto_tail,
            ..ExportOptions::default()
        };
        rig.session.export_audio(options.clone()).unwrap();
        assert_eq!(rig.events.wait_for_export().error, None);
        windfall_codec::decode_file(&options.path).unwrap()
    };
    let whole = export("whole.wav", false);
    let cut = export("cut.wav", true);
    assert_eq!(whole.frames(), 96_000 + 30 * 48_000);
    // The reverb is done long before the thirty seconds are up, and the
    // file ends there. What it holds is the start of the long one.
    assert!(cut.frames() >= 96_000);
    assert!(cut.frames() < 96_000 + 10 * 48_000, "{}", cut.frames());
    assert!(cut.samples() == &whole.samples()[..cut.samples().len()]);
    assert!(peak(&whole.samples()[cut.samples().len()..]) < 1e-4);
}

#[test]
fn effects_and_instruments_are_edited_like_everything_else() {
    let mut rig = Rig::new();
    let (synth, track) = add_synth(&rig);
    let session = rig.session.clone();
    let latency = || session.controller().latency_frames();
    rig.take_patches();

    // An effect is one undo step, and the engine follows it there and back.
    let limiter = add_effect(&rig, track, EffectKind::Limiter);
    let patches = rig.take_patches();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].mixer.is_some() && patches[0].channels.is_none());
    assert_eq!(latency(), LIMITER_LATENCY + SYNTH_LATENCY);
    assert!(session.undo().is_some());
    assert_eq!(latency(), SYNTH_LATENCY);
    assert!(session.redo().is_some());
    assert_eq!(latency(), LIMITER_LATENCY + SYNTH_LATENCY);

    // A knob drag is one step however many values it sends.
    let steps = session.document_snapshot().history.entries.len();
    let settings = EffectKind::Limiter.descriptors();
    let lookahead = settings.iter().position(|info| info.id == "lookaheadMs");
    for value in [6.0, 8.0, 10.0] {
        let command = Command::SetEffectParam {
            track,
            effect: limiter,
            param: lookahead.unwrap() as u32,
            value,
        };
        session.dispatch(command, Some(41)).unwrap();
    }
    let snapshot = session.document_snapshot();
    assert_eq!(snapshot.history.entries.len(), steps + 1);
    assert_eq!(latency(), 480 + SYNTH_LATENCY);

    // The synth's own settings are part of its channel.
    let settings = InstrumentKind::SubtractiveSynth.descriptors();
    let gain = settings.iter().position(|info| info.id == "gain").unwrap();
    let command = Command::SetInstrumentParam {
        channel: synth,
        param: gain as u32,
        value: 0.6,
    };
    let patch = session.dispatch(command, None).unwrap().patch;
    let channels = patch.channels.unwrap();
    let channel = channels.iter().find(|c| c.id == synth).unwrap();
    let ChannelSource::Instrument { params, .. } = &channel.source else {
        panic!("the channel is not an instrument");
    };
    assert_eq!(params.get(gain), Some(0.6));
    assert!(patch.mixer.is_none());

    // Moved to the master and switched off, the limiter still delays by
    // its look-ahead; removed, it does not.
    let moved = Command::MoveEffect {
        track,
        effect: limiter,
        to_track: Some(TrackId::MASTER),
        index: 0,
    };
    session.dispatch(moved, None).unwrap();
    let off = Command::UpdateEffect {
        track: TrackId::MASTER,
        effect: limiter,
        patch: EffectSlotPatch {
            enabled: Some(false),
            mix: None,
        },
    };
    session.dispatch(off, None).unwrap();
    assert_eq!(latency(), 480 + SYNTH_LATENCY);
    let removed = Command::RemoveEffect {
        track: TrackId::MASTER,
        effect: limiter,
    };
    session.dispatch(removed, None).unwrap();
    assert_eq!(latency(), SYNTH_LATENCY);

    // The sampler's commands are refused on the synth with a message the
    // UI can show.
    let wrong = Command::SetSamplerEnvelope {
        id: synth,
        envelope: None,
    };
    let error = session.dispatch(wrong, None).unwrap_err();
    assert!(error.contains("plays an instrument"), "{error}");

    // The synth can be played by hand like any channel.
    session.audition_note_on(synth, 60, 0.9);
    assert!(rms(&rig.run(4_800)) > 0.01);
    session.audition_note_off(synth, 60);
    rig.run(48_000);
    assert_eq!(session.realtime_tick().voices, 0);
}
