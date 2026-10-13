use super::Rig;
use crate::session::recording::{CaptureHandle, Sink};
use windfall_ipc::RecordingSource;
use windfall_project::{ClipContent, Command};
struct FakeCapture {
    frames: u64,
    error: bool,
}
impl CaptureHandle for FakeCapture {
    fn frames(&self) -> u64 {
        self.frames
    }
    fn failed(&self) -> bool {
        self.error
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        if self.error {
            Err("Synthetic dropout".into())
        } else {
            Ok(self.frames)
        }
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
fn synthetic(
    _source: RecordingSource,
    rate: u32,
    mut sink: Sink,
) -> Result<Box<dyn CaptureHandle>, String> {
    assert_eq!(rate, 48000);
    sink(&vec![0.25; 960])?;
    Ok(Box::new(FakeCapture {
        frames: 480,
        error: false,
    }))
}
#[test]
fn recording_attaches_at_chosen_tick_with_undo_and_save_round_trip() {
    let rig = Rig::new();
    let mut before = rig.project();
    let state = rig
        .session
        .recording_start_with(source(), 960, None, synthetic)
        .unwrap();
    assert!(state.active);
    assert_eq!(state.frames, 480);
    assert!(rig.session.project_new().is_err());
    assert!(
        rig.session
            .project_save(Some(&rig.file("blocked.windfall")))
            .is_err()
    );
    assert!(
        rig.session
            .dispatch(
                Command::UpdateSettings {
                    patch: windfall_project::SettingsPatch {
                        tempo_bpm: Some(100.0),
                        ..Default::default()
                    }
                },
                None
            )
            .is_err()
    );
    let result = rig.session.recording_stop().unwrap();
    assert!(!result.created.is_empty());
    let recorded = rig.project();
    let clip = recorded.playlist.clips.last().unwrap();
    assert_eq!(clip.start, 960);
    assert!(matches!(clip.content, ClipContent::Audio { .. }));
    rig.session.undo().unwrap();
    before.next_id = recorded.next_id;
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), recorded);
    let path = rig.file("recorded.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    rig.session.project_open(&path).unwrap();
    assert_eq!(rig.project().playlist, recorded.playlist);
    let info = rig
        .session
        .sample_info_by_id(rig.project().samples.last().unwrap().id)
        .unwrap();
    assert_eq!(info.frames, 480);
}

#[test]
fn imported_punch_region_sets_capture_start_end_and_restores_the_previous_transport_region() {
    let rig = Rig::new();
    rig.session
        .dispatch(
            Command::AddTimelineMarker {
                tick: 960,
                name: "Imported punch".into(),
                kind: windfall_project::MarkerKind::Punch { end: 1920 },
            },
            None,
        )
        .unwrap();
    let before = rig.project();
    assert_eq!(rig.session.controller().timeline_region(), None);
    let state = rig
        .session
        .recording_start_with(source(), 0, None, synthetic)
        .unwrap();
    assert!(state.active);
    assert_eq!(state.frames, 480);
    assert_eq!(
        rig.session.controller().timeline_region(),
        Some(windfall_project::TickRange {
            start: 960,
            end: 1920
        })
    );
    rig.session.recording_stop().unwrap();
    assert_eq!(rig.project().playlist.clips.last().unwrap().start, 960);
    assert_eq!(rig.session.controller().timeline_region(), None);
    rig.session.undo().unwrap();
    assert_eq!(rig.project().playlist.timeline, before.playlist.timeline);
    assert_eq!(rig.project().playlist.clips, before.playlist.clips);
}
#[test]
fn cancel_failed_open_and_dropout_remove_only_owned_take() {
    let rig = Rig::new();
    let before = rig.project();
    let folder = rig.folder.path().join("recordings");
    std::fs::create_dir(&folder).unwrap();
    let old = folder.join("old.wav");
    std::fs::write(&old, b"existing source").unwrap();
    rig.session
        .recording_start_with(source(), 0, None, synthetic)
        .unwrap();
    rig.session.recording_cancel();
    assert_eq!(rig.project(), before);
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    assert!(
        rig.session
            .recording_start_with(source(), 0, None, |_, _, _| Err("Failed input open".into()))
            .is_err()
    );
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    rig.session
        .recording_start_with(source(), 0, None, |_, _, mut sink| {
            sink(&[0.5, 0.5])?;
            Ok(Box::new(FakeCapture {
                frames: 1,
                error: true,
            }))
        })
        .unwrap();
    assert!(rig.session.recording_state().error.is_some());
    assert!(rig.session.recording_stop().is_err());
    assert_eq!(rig.project(), before);
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    assert_eq!(std::fs::read(&old).unwrap(), b"existing source");
}
