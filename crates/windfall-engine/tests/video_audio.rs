use windfall_core::AudioBuffer;
use windfall_engine::{Processor, SamplePool, RenderOptions, render_reporting};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{Project, TrackId, VideoPlacement};

fn fixture() -> (Project, SamplePool) {
    let mut project = Project::new("video mixer");
    let mut track = project.mixer.tracks[0].clone();
    track.id = TrackId(999);
    track.name = "Video sound".into();
    track.volume = 0.5;
    track.output = Some(TrackId::MASTER);
    project.mixer.tracks.push(track);
    project.next_id = project.next_id.max(1000);
    project.settings.stage_media.videos.push(VideoPlacement {
        path: "own.avi".into(), start_tick: 0, length_ticks: 960,
        offset_seconds: 0.0, muted: false, mixer_track: TrackId(999),
    });
    let mut pool = SamplePool::new();
    pool.insert_video_audio("own.avi".into(), AudioBuffer::from_interleaved(48000, 2, vec![0.25; 48000]));
    (project, pool)
}

#[test]
fn video_audio_uses_mixer_fader_and_matches_live_and_export_pcm() {
    let (project, pool) = fixture();
    let export = render_reporting(&project, &pool, &RenderOptions { mode: PlayMode::Song, tail_secs: 0.0, ..Default::default() }, &mut |_| true);
    assert!(export.timeline_error.is_none());
    assert_eq!(export.audio.frames(), 24000);
    assert!(export.audio.samples().iter().all(|sample| (*sample - 0.125).abs() < 1e-6));
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&project, &pool);
    controller.set_transport(TransportPatch { mode: Some(PlayMode::Song), ..Default::default() });
    controller.play();
    let mut live = vec![0.0; 48000];
    for block in live.chunks_mut(14) { processor.process(block); }
    assert_eq!(live, export.audio.samples());
}

#[test]
fn video_audio_mute_and_missing_decoder_cannot_silently_export_success() {
    let (mut project, pool) = fixture();
    let missing = render_reporting(&project, &SamplePool::new(), &RenderOptions { mode: PlayMode::Song, ..Default::default() }, &mut |_| true);
    assert!(missing.audio.samples().is_empty());
    assert!(missing.timeline_error.unwrap().contains("Video audio is unavailable"));
    project.settings.stage_media.videos[0].muted = true;
    let muted = render_reporting(&project, &pool, &RenderOptions { mode: PlayMode::Song, tail_secs: 0.0, ..Default::default() }, &mut |_| true);
    assert_eq!(muted.audio.frames(), 24000);
    assert!(muted.audio.samples().iter().all(|sample| *sample == 0.0));
}
