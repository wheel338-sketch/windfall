//! The existing streaming renderer is the playlist bounce render seam.
use windfall_core::AudioBuffer;
use windfall_engine::{RenderOptions, SamplePool, render_streaming_checked};
use windfall_ipc::PlayMode;
use windfall_project::{
    Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, Lane, Note, NoteId,
    PlaylistTrack, PlaylistTrackId, Project, SampleAsset, SampleId, SamplePath, SamplerSettings,
    TickRange, TrackId,
};

fn audio(sample: SampleId) -> ClipContent {
    ClipContent::Audio {
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
    }
}

#[test]
fn soloed_playlist_span_renders_note_and_audio_into_a_buffer_and_excludes_other_tracks() {
    for pattern in [true, false] {
        let mut project = Project::new("Bounce span");
        project.settings.tempo_bpm = 120.0;
        project.next_id = 100;
        let sample = SampleId(11);
        let channel = ChannelId(10);
        project.samples.push(SampleAsset {
            id: sample,
            name: "Tone".into(),
            path: SamplePath::Project("tone.wav".into()),
        });
        project.channels.push(Channel {
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
        project.playlist.tracks = vec![
            PlaylistTrack {
                id: PlaylistTrackId(50),
                name: "Chosen".into(),
                muted: true,
                solo: false,
                color: 0,
                height: 0,
            },
            PlaylistTrack {
                id: PlaylistTrackId(51),
                name: "Other".into(),
                muted: false,
                solo: true,
                color: 0,
                height: 0,
            },
        ];
        project.playlist.clips = vec![
            Clip {
                id: ClipId(60),
                track: PlaylistTrackId(50),
                start: 960,
                length: 960,
                offset: 0,
                muted: false,
                content: if pattern {
                    ClipContent::Pattern {
                        pattern: project.patterns[0].id,
                    }
                } else {
                    audio(sample)
                },
            },
            Clip {
                id: ClipId(61),
                track: PlaylistTrackId(51),
                start: 960,
                length: 1920,
                offset: 0,
                muted: false,
                content: audio(sample),
            },
        ];
        project.check().unwrap();
        let live = project.clone();
        let mut source = project.clone();
        source.playlist.tracks[0].solo = true;
        source.playlist.tracks[0].muted = false;
        source.playlist.tracks[1].solo = false;
        let mut pool = SamplePool::new();
        pool.insert(
            sample,
            AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 96_000]),
        );
        let options = RenderOptions {
            region: Some(TickRange {
                start: 960,
                end: 1920,
            }),
            mode: PlayMode::Song,
            sample_rate: 48_000,
            ..Default::default()
        };
        let collect = |project: &Project| {
            let mut buffer = Vec::new();
            let result = render_streaming_checked(
                project,
                &pool,
                &options,
                &mut |block| {
                    buffer.extend_from_slice(block);
                    true
                },
                &mut |_| true,
            )
            .unwrap();
            assert!(result.completed);
            assert_eq!(result.frames, 24_000);
            buffer
        };
        let actual = collect(&source);
        assert!(actual.iter().any(|value| value.abs() > 0.01));
        // A muted-clip reference proves the other, audible track contributes nothing.
        let mut reference = source.clone();
        reference.playlist.clips[1].muted = true;
        assert_eq!(actual, collect(&reference));
        reference.playlist.clips[0].muted = true;
        reference.playlist.clips[1].muted = false;
        assert!(collect(&reference).iter().all(|value| *value == 0.0));
        assert_eq!(project, live);
    }
}
