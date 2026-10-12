//! `wf-transport` as one flow through the session: play, stop, record,
//! pattern and song mode, and the song position. The parts are tested on
//! their own in `playback.rs` and `recording.rs`; this is the row's summary
//! end to end on one project, with a synthetic input in place of a device.
use super::{Rig, SAMPLE_RATE, rms};
use crate::session::recording::{CaptureHandle, Sink};
use windfall_ipc::{PlayMode, RecordingSource, TransportPatch};
use windfall_project::{ClipContent, Command};

/// Where the take is placed: the start of the second bar at 4/4.
const TAKE_TICK: u32 = 3_840;
/// Frames of the synthetic take: half a second.
const TAKE_FRAMES: usize = SAMPLE_RATE as usize / 2;

struct Take;
impl CaptureHandle for Take {
    fn frames(&self) -> u64 {
        TAKE_FRAMES as u64
    }
    fn failed(&self) -> bool {
        false
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Ok(TAKE_FRAMES as u64)
    }
}

fn source() -> RecordingSource {
    RecordingSource {
        host: "Fake".into(),
        device: "Fake".into(),
        left: 0,
        right: None,
        armed_tracks: None,
        mixer_tap: None,
        alignment: None,
        loop_recording: None,
        monitor: None,
    }
}

/// An input that delivers a steady level and then ends.
fn steady(
    _source: RecordingSource,
    rate: u32,
    mut sink: Sink,
) -> Result<Box<dyn CaptureHandle>, String> {
    assert_eq!(rate, SAMPLE_RATE);
    // Twice the frames reported, as the session's own recording tests feed it.
    sink(&vec![0.25; TAKE_FRAMES * 2])?;
    Ok(Box::new(Take))
}

fn mode(rig: &Rig, mode: PlayMode) {
    let state = rig
        .session
        .transport_set(TransportPatch {
            mode: Some(mode),
            loop_song: Some(false),
            ..TransportPatch::default()
        })
        .unwrap();
    assert_eq!(state.mode, mode);
}

#[test]
fn the_transport_plays_stops_records_and_switches_between_pattern_and_song() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(0),
                step: 0,
            },
            None,
        )
        .unwrap();
    let start = session.transport_state();
    assert!(!start.playing);
    assert_eq!(start.mode, PlayMode::Pattern);
    rig.events.take();

    // Play the pattern: the step sounds and the position moves.
    assert!(session.transport_play().unwrap().playing);
    let heard = rig.run(SAMPLE_RATE as usize / 4);
    assert!(rms(&heard) > 0.001, "the pattern's step is heard");
    let moved = session.realtime_tick().tick;
    // A quarter of a second at 120 bpm is half a beat.
    assert!((moved - 480.0).abs() < 1.0, "tick {moved}");

    // Stop: silence, and the playhead is back where playback started.
    assert!(!session.transport_stop().playing);
    rig.run(SAMPLE_RATE as usize / 2);
    assert_eq!(rms(&rig.run(4_800)), 0.0);
    assert_eq!(session.realtime_tick().tick, 0.0);

    // Record a take at the second bar. While it runs the project cannot be
    // edited and the transport's mode cannot be changed under it.
    let before_take = rig.project();
    let recording = session
        .recording_start_with(source(), TAKE_TICK, None, steady)
        .unwrap();
    assert!(recording.active);
    assert!(session.recording_state().active);
    assert!(
        session
            .transport_set(TransportPatch {
                mode: Some(PlayMode::Song),
                ..TransportPatch::default()
            })
            .is_err()
    );
    let result = session.recording_stop().unwrap();
    assert!(!session.recording_state().active);
    assert!(!result.created.is_empty());
    let recorded = rig.project();
    let clip = recorded.playlist.clips.last().unwrap();
    assert_eq!(clip.start, TAKE_TICK);
    assert!(matches!(clip.content, ClipContent::Audio { .. }));

    // Song mode plays the playlist: nothing before the take, the take at
    // its tick, and the position is the song's.
    mode(&rig, PlayMode::Song);
    assert!(session.transport_play().unwrap().playing);
    let lead_in = rig.run(SAMPLE_RATE as usize);
    assert_eq!(rms(&lead_in), 0.0, "the pattern is not part of the song");
    session.transport_seek(f64::from(TAKE_TICK));
    let take = rig.run(SAMPLE_RATE as usize / 4);
    assert!(rms(&take) > 0.01, "the recorded take plays in the song");
    let position = session.realtime_tick().tick;
    assert!(
        (position - f64::from(TAKE_TICK) - 480.0).abs() < 1.0,
        "tick {position}"
    );
    assert!(!session.transport_stop().playing);
    rig.run(SAMPLE_RATE as usize / 2);

    // Back in pattern mode the pattern plays again, from its start.
    mode(&rig, PlayMode::Pattern);
    session.transport_seek(0.0);
    assert!(session.transport_play().unwrap().playing);
    assert!(rms(&rig.run(SAMPLE_RATE as usize / 4)) > 0.001);
    assert!(!session.transport_toggle().unwrap().playing);

    // Every change of play state and mode was announced, in order.
    let seen: Vec<(bool, PlayMode)> = rig
        .take_transport_states()
        .iter()
        .map(|state| (state.playing, state.mode))
        .collect();
    assert_eq!(
        seen,
        [
            (true, PlayMode::Pattern),
            (false, PlayMode::Pattern),
            (false, PlayMode::Song),
            (true, PlayMode::Song),
            (false, PlayMode::Song),
            (false, PlayMode::Pattern),
            (true, PlayMode::Pattern),
            (false, PlayMode::Pattern),
        ]
    );

    // The take is one undo step.
    session.undo().unwrap();
    let mut restored = before_take;
    restored.next_id = rig.project().next_id;
    assert_eq!(rig.project(), restored);
    session.redo().unwrap();
    assert_eq!(rig.project(), recorded);
    let path = rig.file("transport-take.windfall");
    session.project_save(Some(&path)).unwrap();
    session.project_new().unwrap();
    session.project_open(&path).unwrap();
    assert_eq!(rig.project().playlist, recorded.playlist);
}

#[test]
fn a_cancelled_recording_leaves_the_transport_and_the_project_as_they_were() {
    let rig = Rig::new();
    let session = &rig.session;
    let project = rig.project();
    let transport = session.transport_state();
    session
        .recording_start_with(source(), 0, None, steady)
        .unwrap();
    session.recording_cancel();
    assert!(!session.recording_state().active);
    assert_eq!(rig.project(), project);
    assert_eq!(session.transport_state(), transport);
    // The transport works again at once.
    assert!(session.transport_play().unwrap().playing);
    assert!(!session.transport_stop().playing);
}
