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

#[test]
fn video_audio_short_loops_wrap_multiple_times_per_callback_without_partition_drift() {
    let (mut project, mut pool) = fixture();
    project.settings.stage_media.videos[0].length_ticks = 1;
    pool.insert_video_audio("own.avi".into(), AudioBuffer::from_interleaved(48000, 2,
        (0..100).flat_map(|frame| [frame as f32 / 1000.0; 2]).collect()));
    let render = |block_frames: usize| {
        let (mut processor, controller) = Processor::new(48000);
        controller.set_project(&project, &pool);
        controller.set_transport(TransportPatch { mode: Some(PlayMode::Song), loop_song: Some(true), ..Default::default() });
        controller.play();
        let mut samples = vec![0.0; 2000];
        for block in samples.chunks_mut(block_frames * 2) { processor.process(block); }
        samples
    };
    let small = render(7);
    let large = render(256);
    for (frame, (a, b)) in small.chunks_exact(2).zip(large.chunks_exact(2)).enumerate() {
        let expected = (frame % 25) as f32 / 2000.0;
        for side in 0..2 {
            assert!((a[side] - expected).abs() < 1e-6, "small block frame {frame}");
            assert!((b[side] - expected).abs() < 1e-6, "large block frame {frame}");
        }
    }
}

#[test]
fn video_mixer_stems_discover_sources_and_isolate_each_soundtrack() {
    let (mut project, mut pool) = fixture();
    let mut second = project.mixer.tracks[1].clone();
    second.id = TrackId(1001); second.name = "Second picture".into(); second.volume = 1.0;
    project.mixer.tracks.push(second); project.next_id = 1002;
    let mut placement = project.settings.stage_media.videos[0].clone();
    placement.path = "second.avi".into(); placement.mixer_track = TrackId(1001);
    project.settings.stage_media.videos.push(placement);
    pool.insert_video_audio("second.avi".into(), AudioBuffer::from_interleaved(48000, 2, vec![0.5; 48000]));
    let mut streams = [Vec::new(), Vec::new(), Vec::new()];
    let rendered = windfall_engine::render_stems(&project, &pool, &RenderOptions { mode: PlayMode::Song, ..Default::default() }, &windfall_engine::StemOptions { mode: windfall_engine::StemMode::ToMaster, tracks: None, include_mix: true, numbered: false }, &mut |index, samples| { streams[index].extend_from_slice(samples); true }, &mut |_| true).unwrap();
    assert!(rendered.completed);
    for (stream, expected) in streams.iter().zip([0.625, 0.125, 0.5]) {
        assert_eq!(stream.len(), 48000);
        assert!(stream.iter().all(|sample| (*sample - expected).abs() < 1e-6));
    }
}
