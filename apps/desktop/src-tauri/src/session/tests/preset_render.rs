//! Native browser/drop jobs share ownership, history and engine admission.
use super::{Rig, SAMPLE_RATE};
use crate::session::PresetRenderRequest;
use std::fs;
use windfall_engine::preset_render::{self as engine, PresetRenderOptions};
use windfall_project::*;

fn synth(rig: &Rig) -> (String, Vec<u8>) {
    let bytes = Preset::of_plugin(
        "Own compact patch",
        PluginPreset {
            tables: None,
            state: PluginPresetState::Instrument {
                params: InstrumentKind::CompactSynth.default_params(),
                asset: None,
            },
        },
    )
    .to_json()
    .unwrap()
    .into_bytes();
    let path = rig.file("Own compact.wfpreset");
    fs::write(&path, &bytes).unwrap();
    (path, bytes)
}
fn options() -> PresetRenderOptions {
    PresetRenderOptions {
        note: 67,
        length_secs: 0.2,
        tail_secs: 0.2,
        stereo: false,
        ..Default::default()
    }
}
fn input(path: String) -> PresetRenderRequest {
    PresetRenderRequest {
        path,
        source: None,
        channel: None,
        index: None,
        browser: None,
        options: options(),
    }
}
fn pool(rig: &Rig) -> Vec<(u32, windfall_core::AudioIdentity)> {
    let state = rig.session.state();
    let mut entries: Vec<_> = state
        .pool
        .iter()
        .map(|(id, audio)| (id.0, audio.identity()))
        .collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}
fn owned(rig: &Rig) -> Vec<std::path::PathBuf> {
    let folder = rig
        .session
        .store()
        .recordings_dir()
        .join("Imported presets");
    let mut files = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn rendered_preset_plugin_packet_carries_exact_original_source_without_pcm_substitution() {
    let rig = Rig::new();
    let (path, bytes) = synth(&rig);
    let ticket = rig.session.preset_render_begin().unwrap();
    rig.session
        .preset_render_file(ticket.request, input(path.clone()))
        .unwrap();
    let (project, pool, directory) = {
        let state = rig.session.state();
        (
            state.document.project().clone(),
            state.pool.clone(),
            state.sample_dir.clone(),
        )
    };
    let program = crate::session::plugin_export::prepare_export_program(
        project.clone(),
        &pool,
        directory.as_deref(),
        std::path::Path::new(super::DEV_FACTORY_DIR),
    )
    .unwrap();
    let recipe = project.settings.preset_renders.last().unwrap();
    assert!(pool.get(recipe.source).is_none());
    assert!(
        !program
            .samples
            .iter()
            .any(|sample| sample.id == recipe.source)
    );
    assert_eq!(program.preset_sources.len(), 1);
    assert_eq!(program.preset_sources[0].id, recipe.source);
    assert_eq!(program.preset_sources[0].bytes, bytes);
    let rendered = program
        .samples
        .iter()
        .find(|sample| sample.id == recipe.sample)
        .unwrap();
    assert_eq!(rendered.data, pool.get(recipe.sample).unwrap().samples());
    let channel = project.channels.iter().find(|channel| matches!(&channel.source, ChannelSource::Sampler(settings) if settings.sample == Some(recipe.sample))).unwrap().id;
    rig.session
        .dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    rig.session
        .dispatch(Command::RemoveSample { id: recipe.sample }, None)
        .unwrap();
    let orphan = rig.project();
    assert!(orphan.settings.preset_renders.is_empty());
    assert!(orphan.sample(recipe.source).is_some());
    let orphan_program = crate::session::plugin_export::prepare_export_program(
        orphan,
        &pool,
        directory.as_deref(),
        std::path::Path::new(super::DEV_FACTORY_DIR),
    )
    .unwrap();
    assert_eq!(orphan_program.preset_sources[0].bytes, bytes);
    fs::remove_file(&path).unwrap();
    for file in owned(&rig) {
        fs::remove_file(file).unwrap();
    }
    let recalled = windfall_plugin::Program::decode(&program.encode().unwrap()).unwrap();
    assert_eq!(recalled.project, project);
    assert_eq!(recalled.preset_sources[0].bytes, bytes);
    assert_eq!(
        recalled
            .samples
            .iter()
            .find(|sample| sample.id == recipe.sample)
            .unwrap()
            .data,
        rendered.data
    );
    let guest = windfall_plugin::GuestEngine::activate(&recalled, f64::from(SAMPLE_RATE)).unwrap();
    assert!(guest.preparation_errors().is_empty());
    let mut damaged = recalled.clone();
    damaged.preset_sources[0].bytes = b"damaged source".to_vec();
    assert!(damaged.validate_preset_sources().is_err());
    let preserved = windfall_plugin::Program::decode(&damaged.encode().unwrap()).unwrap();
    assert_eq!(preserved.preset_sources[0].bytes, b"damaged source");
    assert_eq!(
        preserved
            .samples
            .iter()
            .find(|sample| sample.id == recipe.sample)
            .unwrap()
            .data,
        rendered.data
    );
    assert!(
        !windfall_plugin::GuestEngine::activate(&preserved, f64::from(SAMPLE_RATE))
            .unwrap()
            .preparation_errors()
            .is_empty()
    );
    let mut duplicate = recalled.clone();
    duplicate
        .preset_sources
        .push(duplicate.preset_sources[0].clone());
    assert!(duplicate.validate().is_err());
    let mut oversized = recalled;
    oversized.preset_sources[0].bytes = vec![0; engine::MAX_PRESET_BYTES + 1];
    assert!(oversized.validate().is_err());
}

#[test]
fn windfall_browser_and_sampler_drop_render_pcm_source_history_and_portable_reopen() {
    let rig = Rig::new();
    let (path, bytes) = synth(&rig);
    let baseline = rig.project();
    rig.session
        .browser_add_root(&rig.folder.path().to_string_lossy())
        .unwrap();
    let token = rig.session.library_file(&path).unwrap();
    let ticket = rig.session.preset_render_begin().unwrap();
    assert_eq!(ticket.sample_rate, SAMPLE_RATE);
    let result = rig
        .session
        .preset_render_file(
            ticket.request,
            PresetRenderRequest {
                browser: Some(token),
                ..input(path.clone())
            },
        )
        .unwrap();
    assert_eq!(
        result.created.len(),
        3,
        "source bookkeeping must preserve public ID order"
    );
    let sample = SampleId(result.created[0]);
    let channel = ChannelId(result.created[1]);
    let imported = rig.project();
    assert_eq!(imported.samples.len(), baseline.samples.len() + 2);
    assert_eq!(rig.session.document_snapshot().history.cursor, 1);
    let ChannelSource::Sampler(settings) = &imported.channel(channel).unwrap().source else {
        panic!()
    };
    assert_eq!(settings.root_key, 67);
    let recipe = imported
        .settings
        .preset_renders
        .iter()
        .find(|r| r.sample == sample)
        .unwrap();
    assert_eq!(recipe.settings, options());
    let expected = engine::render(&bytes, options(), SAMPLE_RATE, &mut || true).unwrap();
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        expected.samples()
    );
    rig.session.undo().unwrap();
    assert!(rig.project().settings.preset_renders.is_empty());
    assert!(!rig.has_audio(sample));
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), imported);
    // The ordinary session drop endpoint uses the same prepared import path.
    let drop_result = rig
        .session
        .set_channel_sample_from_file(channel, &path)
        .unwrap();
    let dropped = SampleId(drop_result.created[0]);
    assert_eq!(rig.project().samples.len(), baseline.samples.len() + 4);
    assert_eq!(rig.session.document_snapshot().history.cursor, 2);
    assert!(rig.has_audio(dropped));
    fs::remove_file(&path).unwrap();
    let saved = rig
        .session
        .project_save(Some(&rig.file("portable/Presets.windfall")))
        .unwrap();
    let portable = rig.project();
    let source = portable.sample(recipe.source).unwrap();
    let SamplePath::Project(relative) = &source.path else {
        panic!("source must be carried on first save")
    };
    assert_eq!(
        fs::read(
            std::path::Path::new(&saved)
                .parent()
                .unwrap()
                .join(relative)
        )
        .unwrap(),
        bytes
    );
    let moved = rig
        .session
        .project_save(Some(&rig.file("moved/Presets.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&moved).unwrap();
    assert!(rig.has_audio(sample));
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        expected.samples()
    );
    let reopened = rig.project();
    let rerender = reopened
        .settings
        .preset_renders
        .iter()
        .find(|r| r.sample == sample)
        .unwrap();
    let ticket = rig.session.preset_render_begin().unwrap();
    let result = rig
        .session
        .preset_render_file(
            ticket.request,
            PresetRenderRequest {
                source: Some(rerender.source),
                channel: Some(channel),
                path: String::new(),
                options: rerender.settings,
                index: None,
                browser: None,
            },
        )
        .unwrap();
    assert_eq!(
        rig.session
            .state()
            .pool
            .get(SampleId(result.created[0]))
            .unwrap()
            .samples(),
        expected.samples()
    );
}

#[test]
fn speech_channel_presets_save_text_and_render_the_complete_phrase() {
    let rig = Rig::new();
    let mut doc = Document::new(Project::new("Speech preset"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::TextVoice),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::SetSampleInstrumentAsset {
            channel,
            expected: None,
            asset: Some(SampleInstrumentAsset::Speech {
                text: "Windfall speaks".into(),
                pitch: 52.0,
                duration: 0.6,
                version: 1,
                source: None,
            }),
        },
        None,
    )
    .unwrap();
    let (body, _) = ChannelPreset::capture(doc.project(), channel).unwrap();
    let bytes = Preset::of_channel("Windfall speaks", body, None)
        .to_json()
        .unwrap();
    let path = rig.file("Windfall speaks.wfpreset");
    fs::write(&path, &bytes).unwrap();
    let ticket = rig.session.preset_render_begin().unwrap();
    let controls = PresetRenderOptions {
        note: 60,
        length_secs: 0.1,
        tail_secs: 0.1,
        ..Default::default()
    };
    let result = rig
        .session
        .preset_render_file(
            ticket.request,
            PresetRenderRequest {
                options: controls,
                ..input(path)
            },
        )
        .unwrap();
    let audio = rig
        .session
        .state()
        .pool
        .get(SampleId(result.created[0]))
        .unwrap()
        .clone();
    assert!(audio.frames() >= (SAMPLE_RATE as f32 * 0.6) as usize);
    assert!(audio.samples().iter().any(|s| s.abs() > 1e-5));
    assert_eq!(
        audio.samples(),
        engine::render(bytes.as_bytes(), controls, SAMPLE_RATE, &mut || true)
            .unwrap()
            .samples()
    );
}

#[test]
fn cancelling_before_dsp_after_render_and_before_commit_preserves_project_history_pool_and_files() {
    for point in ["preset-render:rendered", "import:prepared"] {
        let rig = Rig::new();
        let (path, _) = synth(&rig);
        let ticket = rig.session.preset_render_begin().unwrap();
        let before = rig.session.document_snapshot();
        let sources = pool(&rig);
        let files = owned(&rig);
        let held = rig.session.hold(point);
        let work = rig
            .session
            .background(move |session| session.preset_render_file(ticket.request, input(path)));
        held.wait();
        rig.session.preset_render_cancel(ticket.request);
        held.release();
        assert!(work.join().unwrap().unwrap_err().contains("cancelled"));
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(pool(&rig), sources);
        assert_eq!(owned(&rig), files);
    }
    let rig = Rig::new();
    let (path, _) = synth(&rig);
    let ticket = rig.session.preset_render_begin().unwrap();
    let before = rig.session.document_snapshot();
    rig.session.preset_render_cancel(ticket.request);
    assert!(
        rig.session
            .preset_render_file(ticket.request, input(path))
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(owned(&rig).is_empty());
}

#[test]
fn stale_preset_jobs_reject_edits_replacement_source_changes_rate_changes_and_supersession() {
    for point in ["preset-render:rendered", "import:prepared"] {
        for race in [
            "edit",
            "replace",
            "source",
            "source-identity",
            "rate",
            "supersede",
        ] {
            let rig = Rig::new();
            let (path, bytes) = synth(&rig);
            let ticket = rig.session.preset_render_begin().unwrap();
            let held = rig.session.hold(point);
            let path_for_worker = path.clone();
            let work = rig.session.background(move |session| {
                session.preset_render_file(ticket.request, input(path_for_worker))
            });
            held.wait();
            match race {
                "edit" => {
                    rig.session
                        .dispatch(Command::AddPattern { name: None }, None)
                        .unwrap();
                }
                "replace" => {
                    rig.session.project_new().unwrap();
                }
                "source" => {
                    fs::write(path, [bytes, b" ".to_vec()].concat()).unwrap();
                }
                "source-identity" => {
                    fs::rename(&path, rig.file("old-source.wfpreset")).unwrap();
                    fs::write(path, bytes).unwrap();
                }
                "rate" => {
                    crate::sync::lock(&rig.device.status).sample_rate = 44_100;
                }
                "supersede" => {
                    rig.session.preset_render_begin().unwrap();
                }
                _ => unreachable!(),
            }
            let before = rig.session.document_snapshot();
            let sources = pool(&rig);
            held.release();
            assert!(work.join().unwrap().is_err(), "{point}/{race}");
            assert_eq!(rig.session.document_snapshot(), before, "{point}/{race}");
            assert_eq!(pool(&rig), sources);
            assert!(owned(&rig).is_empty());
        }
    }
}

#[test]
fn a_render_ticket_is_claimed_once_and_browser_root_revocation_refuses_installation() {
    for revoke in [false, true] {
        let rig = Rig::new();
        let (path, _) = synth(&rig);
        let root = rig.folder.path().to_string_lossy().into_owned();
        rig.session.browser_add_root(&root).unwrap();
        let token = rig.session.library_file(&path).unwrap();
        let ticket = rig.session.preset_render_begin().unwrap();
        let held = rig.session.hold("preset-render:rendered");
        let worker_path = path.clone();
        let work = rig.session.background(move |session| {
            session.preset_render_file(
                ticket.request,
                PresetRenderRequest {
                    browser: Some(token),
                    ..input(worker_path)
                },
            )
        });
        held.wait();
        let before = rig.session.document_snapshot();
        assert!(
            rig.session
                .preset_render_file(ticket.request, input(path))
                .unwrap_err()
                .contains("already running")
        );
        assert_eq!(rig.session.document_snapshot(), before);
        if revoke {
            rig.session.browser_remove_root(&root).unwrap();
        }
        held.release();
        let result = work.join().unwrap();
        if revoke {
            assert!(result.is_err());
            assert_eq!(rig.session.document_snapshot(), before);
            assert!(owned(&rig).is_empty());
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn malformed_oversized_empty_speech_and_proprietary_sources_fail_without_import_side_effects() {
    let rig = Rig::new();
    let (_, bytes) = synth(&rig);
    let before = rig.session.document_snapshot();
    let sources = pool(&rig);
    for bytes in [
        vec![b' '; engine::MAX_PRESET_BYTES + 1],
        b"{}".to_vec(),
        b"{truncated".to_vec(),
        String::from_utf8(bytes)
            .unwrap()
            .replace("compactSynth", "midiOut")
            .into_bytes(),
    ] {
        let path = rig.file("hostile.wfpreset");
        fs::write(&path, bytes).unwrap();
        assert!(rig.session.add_channel_from_file(&path, None).is_err());
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(pool(&rig), sources);
    }
    for extension in ["syn", "speech"] {
        // Even a missing source produces the specific capability report, and
        // the implementation never attempts to inspect proprietary bytes.
        let path = rig.file(&format!("Unsupported.{extension}"));
        let error = rig.session.add_channel_from_file(&path, None).unwrap_err();
        assert!(error.contains("unsupported"));
        assert!(error.contains(".wfpreset"));
        assert_eq!(rig.session.document_snapshot(), before);
    }
    let mut doc = Document::new(Project::new("Empty speech"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::TextVoice),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let (body, _) = ChannelPreset::capture(doc.project(), channel).unwrap();
    let empty = Preset::of_channel("No phrase", body, None)
        .to_json()
        .unwrap();
    let path = rig.file("Empty.wfpreset");
    fs::write(&path, empty).unwrap();
    assert!(
        rig.session
            .add_channel_from_file(&path, None)
            .unwrap_err()
            .contains("empty")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(pool(&rig), sources);
    assert!(owned(&rig).is_empty());
}
