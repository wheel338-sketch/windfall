use windfall_plugin::{EmbeddedSample, Program};
use windfall_project::{
    Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, ExternalOutputRoute, Lane, Note,
    NoteId, PlaylistTrack, PlaylistTrackId, SampleAsset, SampleId, SamplePath, SamplerSettings,
    TrackId,
};
pub fn program() -> Program {
    let mut program = Program::default();
    program.project.next_id = 20;
    program.project.samples.push(SampleAsset {
        soundfont: false,
        regions: Vec::new(),
        processing: None,
        id: SampleId(2),
        name: "source".into(),
        path: SamplePath::Project("source.wav".into()),
    });
    program.samples.push(EmbeddedSample {
        id: SampleId(2),
        sample_rate: 48_000,
        channels: 2,
        data: (0..4096)
            .flat_map(|frame| {
                let value = 0.125 + (frame % 31) as f32 / 64.0;
                [value, -value]
            })
            .collect(),
    });
    program.project.channels.push(Channel {
        sample_instrument: None,
        sample_playback: None,
        id: ChannelId(3),
        name: "sampler".into(),
        group: String::new(),
        timing: Default::default(),
        voice: Default::default(),
        expression_map: Default::default(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        mixer_track: TrackId::MASTER,
        source: ChannelSource::Sampler(SamplerSettings {
            sample: Some(SampleId(2)),
            ..Default::default()
        }),
    });
    // Project output route chooses port 2, not merely a declared but silent bus.
    program.project.mixer.tracks[0].external_output = Some(ExternalOutputRoute {
        left: 2,
        right: Some(3),
        exclusive: false,
    });
    program.project.patterns[0].lanes.push(Lane {
        channel: ChannelId(3),
        notes: vec![Note {
            id: NoteId(4),
            start: 960,
            length: 480,
            key: windfall_project::DEFAULT_KEY,
            velocity: 1.0,
            pan: 0.0,
            expression: Default::default(),
        }],
    });
    program.project.playlist.tracks.push(PlaylistTrack {
        id: PlaylistTrackId(5),
        name: "Song".into(),
        muted: false,
        solo: false,
        color: 0,
        height: 0,
    });
    program.project.playlist.clips.push(Clip {
        id: ClipId(6),
        track: PlaylistTrackId(5),
        start: 0,
        length: 3840,
        offset: 0,
        muted: false,
        content: ClipContent::Pattern {
            pattern: program.project.patterns[0].id,
        },
    });
    program.validate().unwrap();
    program
}
