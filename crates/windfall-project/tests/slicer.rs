use windfall_core::AudioBuffer;
use windfall_project::{
    slicer::{self, SliceOptions},
    *,
};

fn clip() -> Clip {
    Clip {
        id: ClipId(4),
        track: PlaylistTrackId(3),
        start: 120,
        length: 2880,
        offset: 240,
        muted: true,
        content: ClipContent::Audio {
            sample: SampleId(2),
            mixer_track: TrackId(0),
            output: Default::default(),
            normalize: false,
            gain: 0.7,
            pan: -0.3,
            fade_in: 0,
            fade_out: 0,
            reverse: true,
            pitch: 3.0,
            stretch: ClipStretch::Tape,
        },
    }
}
fn tone() -> AudioBuffer {
    AudioBuffer::from_interleaved(1000, 1, vec![0.5; 4000])
}

#[test]
fn song_aligned_grid_has_only_interior_markers_and_a_visible_reversed_overview() {
    let analysis = slicer::analyze(
        &tone(),
        &clip(),
        120.0,
        0.0,
        SliceOptions::Grid { grid_ticks: PPQ },
    )
    .unwrap();
    assert_eq!(
        analysis.markers.iter().map(|m| m.tick).collect::<Vec<_>>(),
        [840, 1800, 2760]
    );
    assert_eq!(analysis.peaks, vec![0.5; 128]);
    assert_eq!(
        slicer::analyze(
            &tone(),
            &clip(),
            120.0,
            0.0,
            SliceOptions::Grid { grid_ticks: PPQ }
        )
        .unwrap(),
        analysis
    );
}

#[test]
fn transient_sensitivity_detects_stereo_attacks_without_polarity_cancellation() {
    let mut samples = vec![0.0; 8000];
    for (frame, amplitude) in [(500, 1.0), (1000, 0.1), (2000, 1.0)] {
        for f in frame..frame + 30 {
            samples[f * 2] = amplitude;
            samples[f * 2 + 1] = -amplitude;
        }
    }
    let audio = AudioBuffer::from_interleaved(1000, 2, samples);
    let mut clip = clip();
    clip.start = 0;
    clip.offset = 0;
    clip.length = 7680;
    if let ClipContent::Audio { reverse, pitch, .. } = &mut clip.content {
        *reverse = false;
        *pitch = 0.0;
    }
    let low = slicer::analyze(
        &audio,
        &clip,
        120.0,
        0.0,
        SliceOptions::Transients { sensitivity: 0.0 },
    )
    .unwrap();
    let high = slicer::analyze(
        &audio,
        &clip,
        120.0,
        0.0,
        SliceOptions::Transients { sensitivity: 1.0 },
    )
    .unwrap();
    assert_eq!(
        low.markers.iter().map(|m| m.tick).collect::<Vec<_>>(),
        [960, 3840]
    );
    assert_eq!(
        high.markers.iter().map(|m| m.tick).collect::<Vec<_>>(),
        [960, 1920, 3840]
    );
    assert_eq!(
        high,
        slicer::analyze(
            &audio,
            &clip,
            120.0,
            0.0,
            SliceOptions::Transients { sensitivity: 1.0 }
        )
        .unwrap()
    );
}

#[test]
fn reverse_trim_and_tape_pitch_map_candidates_to_visible_clip_time() {
    let mut samples = vec![0.0; 4000];
    // Reverse attack: falling edge in the original becomes rising in playback.
    samples[2470..2500].fill(1.0);
    let audio = AudioBuffer::from_interleaved(1000, 1, samples);
    let mut c = clip();
    c.offset = 480;
    c.length = 2880;
    if let ClipContent::Audio { pitch, .. } = &mut c.content {
        *pitch = 12.0;
    }
    let found = slicer::analyze(
        &audio,
        &c,
        120.0,
        0.0,
        SliceOptions::Transients { sensitivity: 0.5 },
    )
    .unwrap();
    assert_eq!(
        found.markers.iter().map(|m| m.tick).collect::<Vec<_>>(),
        [960]
    );
    assert!(found.peaks[..35].iter().all(|p| *p == 0.0));
}

#[test]
fn slicing_is_one_undo_step_and_round_trips_with_all_linked_settings() {
    let mut project = Project::new("Slice test");
    let c = clip();
    project.samples.push(SampleAsset {
            processing: None,
        id: SampleId(2),
        name: "original".into(),
        path: SamplePath::External("original.wav".into()),
    });
    project.playlist.tracks.push(PlaylistTrack {
        id: PlaylistTrackId(3),
        name: "Audio".into(),
        muted: false,
        solo: false,
        color: 0,
        height: 0,
    });
    project.playlist.clips.push(c.clone());
    project.next_id = 5;
    project.check().unwrap();
    let mut doc = Document::new(project.clone());
    let result = doc
        .dispatch(
            slicer::split_command(&c, &[840, 1800], 120.0, 0.0).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(result.created.len(), 3);
    assert_eq!(doc.history().entries.len(), 1);
    assert_eq!(doc.history().entries[0].label, "Slice audio clip");
    let after = doc.project().clone();
    let slices = &after.playlist.clips;
    assert_eq!(
        slices
            .iter()
            .map(|s| (s.start, s.length, s.offset))
            .collect::<Vec<_>>(),
        [(120, 840, 240), (960, 960, 1080), (1920, 1080, 2040)]
    );
    for slice in slices {
        assert_eq!(slice.content, c.content);
        assert_eq!(slice.track, c.track);
        assert!(slice.muted);
    }
    assert_eq!(after.samples, project.samples);
    assert_eq!(
        file::from_json(&file::to_json(&after).unwrap()).unwrap(),
        after
    );
    doc.undo().unwrap();
    project.next_id = after.next_id;
    assert_eq!(doc.project(), &project);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after);
}

#[test]
fn invalid_markers_settings_and_source_tails_are_rejected_before_a_command_exists() {
    let c = clip();
    for cuts in [
        vec![],
        vec![0],
        vec![2880],
        vec![100, 100],
        vec![200, 100],
        vec![10; 2049],
    ] {
        assert!(slicer::split_command(&c, &cuts, 120.0, 0.0).is_err());
    }
    assert!(slicer::split_command(&c, &[100], 120.0, 0.2).is_err());
    let mut unsupported = c.clone();
    if let ClipContent::Audio { fade_in, .. } = &mut unsupported.content {
        *fade_in = 20;
    }
    assert!(slicer::split_command(&unsupported, &[100], 120.0, 0.0).is_err());
    if let ClipContent::Audio {
        fade_in, stretch, ..
    } = &mut unsupported.content
    {
        *fade_in = 0;
        *stretch = ClipStretch::Spectral {
            ratio: 1.0,
            quality: ClipStretchQuality::Standard,
            formants: false,
        };
    }
    assert!(slicer::split_command(&unsupported, &[100], 120.0, 0.0).is_err());
    let options = SliceOptions::Grid { grid_ticks: PPQ };
    assert!(
        slicer::analyze(
            &AudioBuffer::from_interleaved(1000, 1, vec![]),
            &c,
            120.0,
            0.0,
            options
        )
        .is_err()
    );
    let mut tail = c.clone();
    tail.length = 20_000;
    assert!(slicer::analyze(&tone(), &tail, 120.0, 0.0, options).is_err());
    assert!(
        slicer::analyze(
            &tone(),
            &c,
            120.0,
            0.0,
            SliceOptions::Grid { grid_ticks: 0 }
        )
        .is_err()
    );
    assert!(
        slicer::analyze(
            &tone(),
            &c,
            120.0,
            0.0,
            SliceOptions::Transients {
                sensitivity: f32::NAN
            }
        )
        .is_err()
    );
    assert!(
        slicer::analyze(
            &AudioBuffer::from_interleaved(1000, 1, vec![f32::NAN; 4000]),
            &c,
            120.0,
            0.0,
            options
        )
        .is_err()
    );
}

#[test]
fn silence_and_sub_tick_clips_have_no_candidates() {
    let mut c = clip();
    c.offset = 0;
    c.start = 0;
    c.length = 1;
    let silent = AudioBuffer::from_interleaved(1000, 1, vec![0.0; 10]);
    for options in [
        SliceOptions::Grid {
            grid_ticks: PPQ / 4,
        },
        SliceOptions::Transients { sensitivity: 1.0 },
    ] {
        assert!(
            slicer::analyze(&silent, &c, 120.0, 0.0, options)
                .unwrap()
                .markers
                .is_empty()
        );
    }
}
