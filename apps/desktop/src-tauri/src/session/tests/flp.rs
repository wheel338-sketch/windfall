//! Original, generated FL containers: no proprietary project fixtures.
use super::Rig;
use std::fs;
use windfall_flp::{Channel, ChannelKind, Note, Pattern, Plugin};
use windfall_ipc::FlpImportOptions;
use windfall_project::{Command, SamplePath, SettingsPatch};
#[path = "../../../../../../crates/windfall-flp/tests/common/mod.rs"]
mod fixtures;

fn source(rig: &Rig, sample: Option<&str>) -> String {
    let mut project = fixtures::project(fixtures::MODERN, 96);
    project.channels.push(Channel {
        iid: 0,
        kind: ChannelKind::Generator,
        name: Some("Unhosted instrument".into()),
        plugin: Some(Plugin {
            internal_name: "Unknown synth".into(),
            generator: Some(true),
            state: vec![0, 255, 17, 90],
        }),
        ..Default::default()
    });
    if let Some(path) = sample {
        project.channels.push(Channel {
            iid: 1,
            kind: ChannelKind::Sampler,
            sample_path: Some(path.into()),
            ..Default::default()
        });
    }
    project.patterns.push(Pattern {
        iid: 1,
        notes: vec![Note {
            channel: 0,
            length: 96,
            key: 60,
            velocity: 100,
            fine_pitch: 120,
            pan: 64,
            ..Default::default()
        }],
        ..Default::default()
    });
    let path = rig.file("original.flp");
    fs::write(&path, fixtures::write(&project)).unwrap();
    path
}

#[test]
fn flp_review_is_nonmutating_and_open_is_checked_unsaved_and_retains_opaque_state() {
    let rig = Rig::new();
    let path = source(&rig, Some("missing.wav"));
    let original_bytes = fs::read(&path).unwrap();
    let before = rig.session.document_snapshot();
    let preview = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(preview.retained_plugins, 1);
    assert_eq!(preview.missing_samples.len(), 1);
    assert!(!preview.warnings.is_empty());
    assert!(!preview.report.categories.is_empty());
    let opened = rig.session.flp_open(preview.token).unwrap();
    assert!(opened.dirty);
    assert!(opened.path.is_none());
    opened.project.check().unwrap();
    assert_eq!(opened.project.patterns[0].lanes[0].notes.len(), 1);
    assert_eq!(opened.project.retained_plugins[0].state, [0, 255, 17, 90]);
    let saved = rig
        .session
        .project_save(Some(&rig.file("converted.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    let reloaded = rig.session.project_open(&saved).unwrap();
    assert_eq!(
        reloaded.project.retained_plugins,
        opened.project.retained_plugins
    );
    assert!(!reloaded.dirty);
    assert_eq!(fs::read(path).unwrap(), original_bytes);
}

#[test]
fn flp_failure_cancel_and_intervening_edits_preserve_the_active_project() {
    let rig = Rig::new();
    let path = source(&rig, None);
    let before = rig.session.document_snapshot();
    let bad = rig.file("bad.flp");
    fs::write(&bad, b"not an FL project").unwrap();
    assert!(
        rig.session
            .flp_preview(&bad, &FlpImportOptions::default())
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let preview = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    rig.session.flp_cancel(preview.token);
    assert!(rig.session.flp_open(preview.token).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
    let preview = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("Keep this edit".into()),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let edited = rig.session.document_snapshot();
    assert!(rig.session.flp_open(preview.token).is_err());
    assert_eq!(rig.session.document_snapshot(), edited);
}

#[test]
fn flp_sample_search_loads_unique_matches_but_keeps_ambiguities_recoverable() {
    let rig = Rig::new();
    let path = source(&rig, Some("C:\\lost\\tone.wav"));
    let folder = rig.folder.path().join("samples");
    fs::create_dir(&folder).unwrap();
    let buffer = windfall_core::AudioBuffer::from_interleaved(48_000, 2, vec![0.25; 960]);
    windfall_codec::write_wav(
        folder.join("tone.wav"),
        &buffer,
        windfall_codec::WavSampleFormat::Int24,
    )
    .unwrap();
    let options = FlpImportOptions {
        sample_search_folders: vec![crate::paths::display(&folder)],
        ..Default::default()
    };
    let preview = rig.session.flp_preview(&path, &options).unwrap();
    assert!(preview.missing_samples.is_empty());
    let opened = rig.session.flp_open(preview.token).unwrap();
    assert_eq!(
        opened.project.samples[0].path,
        SamplePath::External(crate::paths::display(&folder.join("tone.wav")))
    );
    fs::create_dir(folder.join("duplicate")).unwrap();
    fs::copy(folder.join("tone.wav"), folder.join("duplicate/tone.wav")).unwrap();
    let preview = rig.session.flp_preview(&path, &options).unwrap();
    assert_eq!(preview.missing_samples.len(), 1);
    assert!(
        preview
            .warnings
            .iter()
            .any(|s| windfall_core::english::english(s).contains("More than one file matches"))
    );
}

#[test]
fn flp_superseded_previews_and_nonproject_containers_never_replace_the_document() {
    let rig = Rig::new();
    let path = source(&rig, None);
    let first = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    let second = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    assert!(rig.session.flp_open(first.token).is_err());
    rig.session.project_new().unwrap();
    let before = rig.session.document_snapshot();
    assert!(rig.session.flp_open(second.token).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
    let mut score = fs::read(path).unwrap();
    score[8] = 0x10;
    let score_path = rig.file("score.flp");
    fs::write(&score_path, score).unwrap();
    assert!(
        windfall_core::english::english(
            &rig.session
                .flp_preview(&score_path, &FlpImportOptions::default())
                .unwrap_err()
        )
        .contains("preset or score")
    );
    let too_many = FlpImportOptions {
        sample_search_folders: vec![rig.file("samples"); 33],
        ..Default::default()
    };
    assert!(rig.session.flp_preview(&score_path, &too_many).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn flp_wav_loop_metadata_is_reviewed_and_survives_save_reopen() {
    use windfall_codec::{
        AudioTags, WavLoop, WavLoopKind, WavMetadata, WavSampleFormat, WavWriter,
    };
    use windfall_project::{ChannelSource, SamplerLoopMode};
    let rig = Rig::new();
    let sample = rig.file("own-loop.wav");
    let metadata = WavMetadata {
        loops: vec![WavLoop {
            cue_id: 1,
            start: 8,
            end: 32,
            kind: WavLoopKind::Alternating,
            play_count: 0,
        }],
        ..Default::default()
    };
    let mut writer = WavWriter::create_with_metadata(
        &sample,
        48000,
        1,
        WavSampleFormat::Float32,
        &AudioTags::default(),
        &metadata,
    )
    .unwrap();
    writer.write(&[0.25; 64]).unwrap();
    writer.finalize().unwrap();
    let mut project = fixtures::project(fixtures::MODERN, 96);
    project.channels.push(Channel {
        iid: 0,
        kind: ChannelKind::Sampler,
        sample_path: Some(sample),
        sampler_flags: Some(8),
        ..Default::default()
    });
    let path = rig.file("own-loop.flp");
    fs::write(&path, fixtures::write(&project)).unwrap();
    let before = rig.session.document_snapshot();
    let preview = rig
        .session
        .flp_preview(&path, &FlpImportOptions::default())
        .unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(
        preview
            .warnings
            .iter()
            .any(|warning| windfall_core::english::english(warning).contains("smpl chunk"))
    );
    let opened = rig.session.flp_open(preview.token).unwrap();
    let ChannelSource::Sampler(settings) = &opened.project.channels[0].source else {
        panic!("expected sampler")
    };
    assert_eq!(settings.loop_mode, SamplerLoopMode::PingPong);
    assert_eq!((settings.loop_start, settings.loop_end), (0.125, 0.5));
    let saved = rig
        .session
        .project_save(Some(&rig.file("own-loop.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    let reloaded = rig.session.project_open(&saved).unwrap();
    assert_eq!(reloaded.project.channels, opened.project.channels);
}
