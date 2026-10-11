//! Playlist solo through the public renderer, including automation's song clock.

use windfall_core::AudioBuffer;
use windfall_engine::{RenderOptions, SamplePool, render};
use windfall_ipc::PlayMode;
use windfall_project::{
    Automation, AutomationId, AutomationPoint, AutomationTarget, Channel, ChannelId, ChannelSource,
    Clip, ClipContent, ClipId, Lane, Note, NoteId, PlaylistTrack, PlaylistTrackId, Project,
    SampleAsset, SampleId, SamplePath, SamplerSettings, TrackId,
};

#[derive(Clone, Copy)]
enum Kind {
    Pattern,
    Audio,
    Automation,
}

fn fixture(kind: Kind) -> (Project, SamplePool) {
    let mut project = Project::new("Playlist track solo");
    project.settings.tempo_bpm = 120.0;
    project.next_id = 100;
    let sample = SampleId(11);
    let channel = ChannelId(10);
    project.samples.push(SampleAsset {
        regions: Vec::new(),
        id: sample,
        name: "Tone".into(),
        path: SamplePath::Project("tone.wav".into()),
    });
    project.channels.push(Channel {
                sample_playback: None,
        id: channel,
        name: "Tone".into(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        group: String::new(),
        voice: Default::default(),
        expression_map: Default::default(),
        timing: Default::default(),
        mixer_track: TrackId::MASTER,
        source: ChannelSource::Sampler(SamplerSettings {
            sample: Some(sample),
            ..Default::default()
        }),
    });
    project.patterns[0].lanes.push(Lane {
        channel,
        notes: vec![Note {
            id: NoteId(12),
            start: 0,
            length: 960,
            key: windfall_project::DEFAULT_KEY,
            velocity: 1.0,
            pan: 0.0,
            expression: Default::default(),
        }],
    });
    for index in 0..2 {
        let track = PlaylistTrackId(50 + index);
        project.playlist.tracks.push(PlaylistTrack {
            id: track,
            name: format!("Track {index}"),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        });
        let content = match kind {
            Kind::Pattern => ClipContent::Pattern {
                pattern: project.patterns[0].id,
            },
            Kind::Audio => ClipContent::Audio {
                sample,
                mixer_track: TrackId::MASTER,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,
                stretch: Default::default(),
            },
            Kind::Automation => {
                let automation = AutomationId(80 + index);
                project.automations.push(Automation {
                    id: automation,
                    name: format!("Tempo {index}"),
                    color: 0,
                    target: AutomationTarget::Tempo,
                    points: vec![AutomationPoint {
                        tick: 0,
                        value: ((if index == 0 { 240.0 } else { 60.0 }) - 10.0) / 512.0,
                        curve: 0.0,
                        hold: true,
                    }],
                });
                ClipContent::Automation { automation }
            }
        };
        project.playlist.clips.push(Clip {
            id: ClipId(60 + index),
            track,
            start: index * 960,
            length: 960,
            offset: 0,
            muted: false,
            content,
        });
    }
    project.check().unwrap();
    let mut pool = SamplePool::new();
    pool.insert(
        sample,
        AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 96_000]),
    );
    (project, pool)
}

fn assert_solo_playback(kind: Kind) {
    let (mut project, pool) = fixture(kind);
    let options = RenderOptions {
        mode: PlayMode::Song,
        sample_rate: 48_000,
        block_frames: 480,
        tail_secs: 0.0,
        ..Default::default()
    };
    for (solo_a, muted_a, solo_b, muted_b, audible) in [
        (false, false, false, false, vec![0, 1]),
        (true, false, false, false, vec![0]),
        (true, true, false, false, vec![]),
        (false, true, false, false, vec![1]),
        (true, false, true, false, vec![0, 1]),
        (true, true, true, false, vec![1]),
    ] {
        project.playlist.tracks[0].solo = solo_a;
        project.playlist.tracks[0].muted = muted_a;
        project.playlist.tracks[1].solo = solo_b;
        project.playlist.tracks[1].muted = muted_b;
        let stored = project.clone();
        // An explicit clip-mute reference keeps the same song length and
        // makes the expected audible tracks independent of the solo rule.
        let mut reference = project.clone();
        for track in &mut reference.playlist.tracks {
            track.solo = false;
            track.muted = false;
        }
        for clip in &mut reference.playlist.clips {
            clip.muted = !audible.contains(&(clip.track.0 - 50));
        }
        let expected = render(&reference, &pool, &options, &mut |_| true);
        let actual = render(&project, &pool, &options, &mut |_| true);
        assert_eq!(actual.samples(), expected.samples());
        assert_eq!(project, stored);
        match kind {
            Kind::Automation => {
                // Tempo changes prove whether automation clips were compiled,
                // including when mute wins over the only solo track.
                let frames = match audible.as_slice() {
                    [0, 1] => 60_000,
                    [0] => 24_000,
                    [1] => 72_000,
                    [] => 48_000,
                    _ => unreachable!(),
                };
                assert_eq!(actual.frames(), frames);
            }
            Kind::Pattern | Kind::Audio => {
                assert_eq!(actual.frames(), 48_000);
                assert_eq!(
                    actual.samples().iter().any(|sample| sample.abs() > 0.01),
                    !audible.is_empty()
                );
            }
        }
    }
}

#[test]
fn playlist_track_solo_filters_pattern_clips_and_mute_wins() {
    assert_solo_playback(Kind::Pattern);
}

#[test]
fn playlist_track_solo_filters_audio_clips_and_mute_wins() {
    assert_solo_playback(Kind::Audio);
}

#[test]
fn playlist_track_solo_filters_automation_clips_and_mute_wins() {
    assert_solo_playback(Kind::Automation);
}
