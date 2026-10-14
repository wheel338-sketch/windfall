//! Independent synthetic streams; literal public offsets and independently
//! calculated numbers. No third-party project or preset is distributed.
mod common;
use common::Events;
use windfall_flp::{ConvertOptions, FileFormat, Header, import};
use windfall_project::{ChannelSource, MarkerKind};

fn stream() -> Events {
    let mut e = Events::new(false);
    e.text(199, "9.0.0");
    e.word(66, 120);
    e.word(64, 0);
    e.byte(21, 0);
    e
}
fn imported(e: &Events) -> windfall_flp::Conversion {
    let file = common::file(
        Header {
            format: FileFormat::Project,
            channel_count: 1,
            ppq: 96,
        },
        &e.bytes,
    );
    let c = import(&file, &ConvertOptions::default()).unwrap();
    c.project.check().unwrap();
    c
}
fn word(b: &mut [u8], offset: usize, v: i32) {
    b[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
}
fn effect(name: &str, b: &[u8]) -> windfall_flp::Conversion {
    let mut e = stream();
    e.text(204, "Master");
    e.text(201, name);
    e.data(213, b);
    e.word(98, 0);
    e.dword(147, u32::MAX);
    imported(&e)
}
fn instrument(name: &str, b: &[u8]) -> windfall_flp::Conversion {
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, name);
    e.data(213, b);
    imported(&e)
}
fn instrument_value(c: &windfall_flp::Conversion, id: &str) -> f32 {
    let ChannelSource::Instrument { params, .. } = c.project.channels[0].source else {
        panic!("instrument")
    };
    params
        .get(
            params
                .kind()
                .descriptors()
                .iter()
                .position(|p| p.id == id)
                .unwrap(),
        )
        .unwrap()
}
fn effect_value(c: &windfall_flp::Conversion, id: &str) -> f32 {
    let p = c.project.mixer.tracks[0].effects[0].params;
    p.get(
        p.kind()
            .descriptors()
            .iter()
            .position(|p| p.id == id)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn native_instrument_words_translate_with_independent_numbers_and_reopen_exact_blobs() {
    // Pluck: .1 + half of 11.9 seconds. Bass: documented full 65535 range.
    for (name, len, offset, raw, target, expected) in [
        ("Plucked!", 20, 0, 128, "decaySeconds", 6.05),
        ("BooBass", 16, 4, 65535, "body", 1.0),
        ("BooBass", 12, 8, 65535, "tone", 1.0),
        (
            "Fruity DX10",
            92,
            4,
            32768,
            "operators.0.envelope.attackMs",
            78.125,
        ),
        ("Fruit Kick", 28, 16, 128, "decayMs", 755.0),
        ("Fruity Kick", 32, 20, 32, "click", 0.5),
        ("Fruity Granulizer", 60, 4, 250, "grainSizeMs", 125.5),
        ("Fruity Slicer", 32, 28, 64, "releaseMs", 50.0),
        ("Slicex", 40, 12, 6400, "level", 0.5),
    ] {
        let mut b = vec![0; len];
        word(&mut b, offset, raw);
        let c = instrument(name, &b);
        assert!(
            (instrument_value(&c, target) - expected).abs() < 0.0001,
            "{name}"
        );
        assert!(common::words(&c.report).contains(name));
        assert_eq!(c.project.retained_plugins[0].state, b);
        let json = windfall_project::file::to_json(&c.project).unwrap();
        assert_eq!(windfall_project::file::from_json(&json).unwrap(), c.project);
    }
}

#[test]
fn dx10_public_power_envelopes_ratios_levels_and_algorithm_are_not_default_patches() {
    let mut b = vec![0; 92];
    for at in [4, 8, 12, 16, 24, 28, 32, 36, 60, 68, 72, 76, 80] {
        word(&mut b, at, 32768);
    }
    word(&mut b, 88, 0);
    let c = instrument("Fruity DX10", &b);
    for (id, want) in [
        ("algorithm", 2.0),
        ("operators.0.ratio", 4.0),
        ("operators.0.envelope.attackMs", 78.125),
        ("operators.0.envelope.decayMs", 1250.0),
        ("operators.0.envelope.sustain", 0.25),
        ("operators.0.envelope.releaseMs", 625.0),
        ("operators.1.ratio", 20.0),
        ("operators.1.level", 0.25),
        ("operators.1.envelope.decayMs", 1250.0),
        ("operators.1.envelope.sustain", 0.25),
        ("operators.1.envelope.releaseMs", 625.0),
        ("operators.2.ratio", 20.0),
        ("operators.3.level", 0.0),
    ] {
        assert_eq!(instrument_value(&c, id), want, "{id}");
    }
    assert!(common::words(&c.report).contains("mod_fine"));
}

#[test]
fn all_remaining_fixed_effect_layouts_have_numeric_translation_and_named_reports() {
    for (name, len, offset, raw, target, want) in [
        ("Fruity Stereo Shaper", 36, 8, 6400, "ll", 0.5),
        ("Fruity Soft Clipper", 8, 0, 127, "ceilingDb", 0.0),
        ("Fruity Fast Dist", 20, 0, 96, "driveDb", 18.0),
        ("Fruity Fast LP", 12, 4, 512, "cutoffHz", 1549.1943),
        ("Fruity Free Filter", 21, 16, 512, "gainDb", 9.0),
        ("Fruity Bass Boost", 12, 8, 512, "gainDb", 9.0),
        ("Fruity Blood Overdrive", 36, 4, 5000, "preBandHz", 2100.0),
        ("Fruity Blood Overdrive", 32, 0, 5000, "preBandHz", 2100.0),
        ("Fruity Chorus", 52, 4, 512, "delayMs", 15.5),
        ("Fruity Flanger", 52, 12, 2500, "rateHz", 0.0390625),
        ("Fruity Phaser", 40, 4, 2500, "rateHz", 2.5),
        ("Fruity Flangus", 32, 8, 512, "rate1Hz", 2.5),
        ("Fruity 7 Band EQ", 32, 4, -600, "gainsDb.0", -6.0),
        ("Fruity Stereo Enhancer", 24, 0, -64, "balance", -0.5),
        ("Soundgoodizer", 12, 8, 500, "amount", 0.5),
        ("Soundgoodizer", 8, 4, 500, "amount", 0.5),
        ("Transient Processor", 48, 4, 120, "attack", 0.5),
        ("Frequency Shifter", 48, 12, 35000, "shiftHz", 156.25),
        ("Spreader", 30, 4, 5000, "amount", 0.5),
        ("Fruity PanOMatic", 24, 0, 64, "pan", -0.5),
        ("Fruity Limiter", 169, 12, 1000, "ceilingDb", -12.0),
        ("Fruity Delay 3", 108, 60, 3000, "feedback", 0.475),
        ("Vintage Chorus", 72, 12, 122880, "rateHz", 2.5),
        ("Vintage Phaser", 136, 28, 122880, "rateHz", 2.5),
        (
            "Fruity Multiband Compressor",
            124,
            100,
            500,
            "low.ratio",
            1.75531,
        ),
        ("Tuner", 20, 4, 3000, "referenceHz", 440.0),
        ("Pitch Shifter", 60, 4, -700, "semitones", -7.0),
    ] {
        let mut b = vec![0; len];
        word(&mut b, offset, raw);
        let c = effect(name, &b);
        assert!(
            (effect_value(&c, target) - want).abs() < 0.003,
            "{name}: {} != {want}",
            effect_value(&c, target)
        );
        let report = common::words(&c.report);
        assert!(report.contains(name), "{name}");
        assert!(report.contains("public native state"), "{name}: {report}");
        assert_eq!(c.project.retained_plugins[0].state, b);
    }
}

#[test]
fn native_enabled_and_mute_words_affect_the_actual_imported_slots() {
    let c = effect("Fruity Center", &[0; 4]);
    assert!(!c.project.mixer.tracks[0].effects[0].enabled);
    let c = effect("Fruity Center", &[1, 0, 0, 0]);
    assert!(c.project.mixer.tracks[0].effects[0].enabled);
    let c = effect("Fruity Mute 2", &[0; 12]);
    assert_eq!(effect_value(&c, "left"), 0.0);
    assert_eq!(effect_value(&c, "right"), 1.0);
    let mut b = vec![0; 8];
    word(&mut b, 4, 1024);
    let c = effect("Fruity Phase Inverter", &b);
    assert_eq!(effect_value(&c, "left"), 1.0);
    assert_eq!(effect_value(&c, "right"), 1.0);
}

#[test]
fn sslf_states_nested_delay_banks_and_float_arrays_use_the_public_offsets() {
    let mut payload = vec![0; 1376];
    payload[1284..1288].copy_from_slice(&0.6_f32.to_le_bytes());
    payload[1308..1312].copy_from_slice(&80_f32.to_le_bytes());
    let mut state = b"SSLF".to_vec();
    state.extend((payload.len() as u32).to_le_bytes());
    state.extend(payload);
    let c = instrument("BassDrum", &state);
    assert_eq!(instrument_value(&c, "level"), 0.6);
    assert_eq!(instrument_value(&c, "pitchHz"), 80.0);
    let mut payload = vec![0; 152];
    payload[96..100].copy_from_slice(&0.5_f32.to_le_bytes());
    let mut state = b"SSLF".to_vec();
    state.extend(152_u32.to_le_bytes());
    state.extend(payload);
    let c = effect("Pitcher", &state);
    assert!((effect_value(&c, "tuningHz") - 440.75).abs() < 0.001);
    let mut b = vec![0; 792];
    word(&mut b, 4, 64);
    word(&mut b, 24, 80);
    b[565] = 1;
    let c = effect("Fruity Delay Bank", &b);
    assert_eq!(effect_value(&c, "dry"), 0.5);
    assert_eq!(effect_value(&c, "units.0.inputGain"), 1.0);
    assert_eq!(effect_value(&c, "units.0.enabled"), 1.0);
    let mut b = vec![0; 1544];
    b[8..12].copy_from_slice(&120_f32.to_le_bytes());
    let c = effect("Multiband Delay", &b);
    // The first of sixteen source bands is band 1 of bank 1, not the legacy
    // three-band "low" control this expectation named before round seven.
    assert_eq!(effect_value(&c, "banks.0.timeMs.0"), 120.0);
    assert!(common::words(&c.report).contains("bank_8/pan.15"));
}

#[test]
fn variable_vocoder_and_waveshaper_records_are_bounded_and_sampled_numerically() {
    let mut b = vec![0; 69];
    word(&mut b, 4, 1);
    word(&mut b, 21 + 24, 32768);
    let c = effect("Fruity Vocoder", &b);
    assert!((effect_value(&c, "attackMs") - 50.05).abs() < 0.001);
    let mut b = vec![0; 34];
    b[16] = 1;
    word(&mut b, 30, 2);
    for (dx, y) in [(0.0_f64, -1.0_f64), (1.0, 1.0)] {
        b.extend(dx.to_le_bytes());
        b.extend(y.to_le_bytes());
        b.extend([0; 8]);
    }
    let c = effect("Fruity Waveshaper", &b);
    for (id, want) in [
        ("curve.0", -1.0),
        ("curve.8", -0.5),
        ("curve.16", 0.0),
        ("curve.24", 0.5),
        ("curve.32", 1.0),
    ] {
        assert_eq!(effect_value(&c, id), want);
    }
    let mut hostile = b.clone();
    word(&mut hostile, 30, i32::MAX);
    let c = effect("Fruity Waveshaper", &hostile);
    assert_eq!(effect_value(&c, "curve.16"), 0.0);
}

#[test]
fn numbered_native_effect_recording_and_clip_controls_share_physical_units() {
    let mut e = stream();
    e.word(65, 1);
    let mut control = Vec::new();
    control.extend(0_u32.to_le_bytes());
    control.extend(0x20008000_u32.to_le_bytes());
    control.extend(512_i32.to_le_bytes());
    e.data(223, &control);
    e.word(64, 1);
    e.byte(21, 5);
    let mut curve = vec![0; 21];
    word(&mut curve, 17, 2);
    for (dx, v) in [(0.0_f64, 0.5_f64), (1.0, 1.0)] {
        curve.extend(dx.to_le_bytes());
        curve.extend(v.to_le_bytes());
        curve.extend([0; 8]);
    }
    e.data(234, &curve);
    let mut link = vec![0; 20];
    word(&mut link, 2, 1);
    word(&mut link, 8, 0x20008000);
    e.data(227, &link);
    e.text(204, "Master");
    e.text(201, "Fruity Fast LP");
    e.data(213, &[0; 12]);
    e.word(98, 0);
    e.dword(147, u32::MAX);
    e.word(99, 0);
    let c = imported(&e);
    let event = &c.project.patterns[0].events[0];
    let clip = &c.project.automations[0];
    assert_eq!(event.target, clip.target);
    let range = c.project.automation_range(&event.target).unwrap();
    assert!((range.value(event.points[0].value) - 1549.1943).abs() < 0.003);
    assert!((range.value(clip.points[0].value) - 1549.1943).abs() < 0.003);
}

#[test]
fn instrument_numbering_and_controller_formula_smoothing_have_specific_source_evidence() {
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, "BooBass");
    e.data(213, &[0; 16]);
    e.word(65, 1);
    let mut event = Vec::new();
    event.extend(0_u32.to_le_bytes());
    event.extend(0x8001_u32.to_le_bytes());
    event.extend(32768_i32.to_le_bytes());
    e.data(223, &event);
    let mut link = vec![0; 20];
    word(&mut link, 2, 0x20000000);
    word(&mut link, 8, 0x8001);
    word(&mut link, 12, 8);
    word(&mut link, 16, 469);
    e.data(227, &link);
    e.data(226, &[1, 2, 3, 4]);
    let c = imported(&e);
    let report = common::words(&c.report);
    for text in [
        "Instrument BooBass parameter 1",
        "SCOPE:",
        "source 0x20000000",
        "smoothing=469",
        "No serialized formula field",
        "MIDI controller event 226",
    ] {
        assert!(report.contains(text), "{report}");
    }
    assert!(c.project.patterns[0].events.is_empty());
    assert!(!c.project.retained_source_data[0].data.is_empty());
}

#[test]
fn channel_pitch_range_timing_anchors_and_full_portamento_bypass_are_translated() {
    for (gate, shift, bypass, want_gate, want_shift) in [
        (450, 1024, false, 30, 960),
        (1446, 0, false, 3840, 0),
        (1446, 0, true, 0, 0),
        (900, 512, false, 0, 0),
    ] {
        let mut e = stream();
        e.byte(21, 2);
        e.text(201, "Fruity Wrapper");
        let mut wrapper = 10_u32.to_le_bytes().to_vec();
        wrapper.extend(1_u32.to_le_bytes());
        wrapper.extend(12_u64.to_le_bytes());
        wrapper.extend((-1_i32).to_le_bytes());
        wrapper.extend((-1_i32).to_le_bytes());
        wrapper.extend(12_u32.to_le_bytes());
        e.data(213, &wrapper);
        e.word(89, shift);
        let mut p = vec![0; 112];
        p[62] = u8::from(bypass);
        p[64..66].copy_from_slice(&(gate as u16).to_le_bytes());
        e.data(215, &p);
        if bypass {
            let mut poly = vec![0; 9];
            poly[8] = 2;
            e.data(221, &poly);
        }
        let c = imported(&e);
        let ch = &c.project.channels[0];
        assert_eq!(ch.expression_map.bend_range_semitones, 12.0);
        assert_eq!(ch.timing.gate_ticks, want_gate);
        assert_eq!(ch.timing.shift_ticks, want_shift);
        if gate == 900 {
            let report = common::words(&c.report);
            assert!(
                report.contains("SCOPE:")
                    && report.contains("gate=900")
                    && report.contains("time shift=Some(512)")
            );
        }
    }
}

#[test]
fn simsynth_filter_and_independent_modulation_use_public_converter_equations() {
    let mut b = vec![0; 516];
    for (i, v) in [
        (22, 0.5_f64),
        (24, 0.3),
        (26, 0.5),
        (27, 0.5),
        (28, 0.5),
        (29, 0.75),
        (30, 0.5),
        (41, 0.25),
        (42, 0.5),
        (43, 0.75),
        (44, 0.5),
    ] {
        b[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    word(&mut b, 484, 1);
    word(&mut b, 492, 1);
    let c = instrument("SimSynth", &b);
    assert!((instrument_value(&c, "cutoffHz") - 2637.0205).abs() < 0.01);
    assert_eq!(instrument_value(&c, "resonance"), 0.4);
    assert_eq!(instrument_value(&c, "envelopeAmount"), 0.0);
    let env = c.project.channels[0].voice.envelopes;
    assert!(env.filter.enabled);
    assert_eq!(
        (
            env.filter.attack_ms,
            env.filter.decay_ms,
            env.filter.sustain,
            env.filter.release_ms,
            env.filter.depth
        ),
        (62.5, 500.0, 0.75, 500.0, 3.2)
    );
    assert_eq!(env.lfos.cutoff.rate_hz, 1.0);
    assert_eq!(env.lfos.cutoff.depth, 0.5);
}

fn marker(e: &mut Events, kind: u32, tick: u32, name: &str) {
    e.dword(148, (kind << 24) | tick);
    e.text(205, name);
}
#[test]
fn start_punch_regions_song_loop_extents_suppressed_pauses_and_unpaired_edges_have_fixtures() {
    let mut e = stream();
    e.word(65, 1);
    e.word(99, 0);
    let mut item = vec![0; 32];
    word(&mut item, 8, 384);
    item[4..6].copy_from_slice(&20480_u16.to_le_bytes());
    item[6..8].copy_from_slice(&20481_u16.to_le_bytes());
    item[12..14].copy_from_slice(&198_u16.to_le_bytes());
    word(&mut item, 24, -1);
    word(&mut item, 28, -1);
    e.data(233, &item);
    marker(&mut e, 5, 24, "Start");
    marker(&mut e, 9, 48, "In");
    marker(&mut e, 10, 96, "Out");
    marker(&mut e, 4, 120, "Song loop");
    marker(&mut e, 3, 144, "Suppressed");
    let c = imported(&e);
    let t = &c.project.playlist.timeline;
    assert_eq!(t.start_tick(), Some(240));
    assert_eq!(
        t.punch_at(0),
        Some(windfall_project::TickRange {
            start: 480,
            end: 960
        })
    );
    assert_eq!(
        t.punch_at(720),
        Some(windfall_project::TickRange {
            start: 720,
            end: 960
        })
    );
    assert_eq!(t.punch_at(960), None);
    assert_eq!(
        t.markers
            .iter()
            .find(|m| m.name == "Song loop")
            .unwrap()
            .kind,
        MarkerKind::Loop { end: 3840 }
    );
    assert_eq!(
        t.markers
            .iter()
            .find(|m| m.name == "Suppressed")
            .unwrap()
            .kind,
        MarkerKind::Named
    );
    assert!(common::words(&c.report).contains("suppressed"));
    let mut e = stream();
    e.word(99, 0);
    marker(&mut e, 9, 48, "Unpaired in");
    marker(&mut e, 10, 0, "Earlier out");
    marker(&mut e, 4, 1000, "Empty loop");
    let c = imported(&e);
    assert!(c.project.playlist.timeline.punch_at(0).is_none());
    assert!(common::words(&c.report).contains("no later positive punch-out"));
    assert!(common::words(&c.report).contains("no positive source endpoint"));
}

#[test]
fn note_edit_groups_are_independent_of_color_and_survive_history_and_json() {
    let mut e = stream();
    e.word(65, 1);
    let mut records = Vec::new();
    for (key, color, group) in [(72_u16, 0_u8, 7_u16), (60, 1, 7), (67, 0, 0)] {
        let mut n = vec![0; 24];
        word(&mut n, 8, 96);
        n[12..14].copy_from_slice(&key.to_le_bytes());
        n[14..16].copy_from_slice(&group.to_le_bytes());
        n[16] = 120;
        n[18] = 64;
        n[19] = color;
        n[20] = 64;
        n[21] = 100;
        n[22] = 128;
        n[23] = 128;
        records.extend(n);
    }
    e.data(224, &records);
    let c = imported(&e);
    let groups = &c.project.note_edit_groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group, 7);
    assert_eq!(groups[0].notes.len(), 2);
    let lane = &c.project.patterns[0].lanes[0];
    let members: Vec<_> = lane
        .notes
        .iter()
        .filter(|n| groups[0].notes.contains(&n.id))
        .map(|n| (n.key, n.expression.color_group))
        .collect();
    assert!(members.contains(&(72, Some(0))) && members.contains(&(60, Some(1))));
    let mut d = windfall_project::Document::new(c.project.clone());
    d.dispatch(
        windfall_project::Command::SetNoteEditGroups { groups: Vec::new() },
        None,
    )
    .unwrap();
    assert!(d.project().note_edit_groups.is_empty());
    assert!(d.undo().is_some());
    assert_eq!(d.project().note_edit_groups, *groups);
    assert!(d.redo().is_some());
    assert!(d.project().note_edit_groups.is_empty());
    assert_eq!(
        windfall_project::file::from_json(&windfall_project::file::to_json(&c.project).unwrap())
            .unwrap(),
        c.project
    );
}

#[test]
fn native_slicer_references_custom_keys_frame_units_reverse_pitch_and_names_survive_reopen() {
    let mut b = vec![0; 32];
    b[4..8].copy_from_slice(&4_f32.to_le_bytes());
    b[8..12].copy_from_slice(&120_f32.to_le_bytes());
    word(&mut b, 12, 700);
    word(&mut b, 24, 64);
    word(&mut b, 28, 64);
    let path = b"synthetic.wav";
    b.push(path.len() as u8);
    b.extend(path);
    b.extend(2_u32.to_le_bytes());
    for (name, start, key, reverse) in [("First", 0_u32, 64_i32, false), ("Second", 100, 66, true)]
    {
        b.push(name.len() as u8);
        b.extend(name.as_bytes());
        b.extend(start.to_le_bytes());
        b.extend(key.to_le_bytes());
        b.extend([0; 4]);
        b.push(u8::from(reverse));
    }
    b.push(0);
    b.extend(60_u32.to_le_bytes());
    b.extend([0; 10]);
    let mut c = instrument("Fruity Slicer", &b);
    assert_eq!(c.native_slices.len(), 1);
    let sample = c.native_slices[0].sample;
    assert_eq!(
        c.project.samples[0].path,
        windfall_project::SamplePath::External("synthetic.wav".into())
    );
    let warnings = windfall_flp::convert::resolve_native_slices(
        &mut c.project,
        &c.native_slices,
        &[(sample, 200, 2)],
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let asset = c.project.channels[0].sample_playback.as_ref().unwrap();
    assert_eq!(asset.slices.len(), 3);
    assert_eq!((asset.source_bpm, asset.source_beats), (120.0, 4.0));
    assert_eq!(
        (
            asset.slices[0].start,
            asset.slices[0].end,
            asset.slices[0].pitch,
            asset.slices[0].attack_ms,
            asset.slices[0].release_ms
        ),
        (0.0, 0.5, 7.0, 50.0, 50.0)
    );
    assert_eq!(asset.slices[1].gain, 0.0);
    assert!(asset.slices[2].reverse);
    assert_eq!(asset.slices[2].label, "Second");
    assert_eq!(asset.slices[2].start, 0.5);
    assert_eq!(instrument_value(&c, "baseKey"), 64.0);
    c.project.check().unwrap();
    assert_eq!(
        windfall_project::file::from_json(&windfall_project::file::to_json(&c.project).unwrap())
            .unwrap(),
        c.project
    );
}

#[test]
fn channel_volume_lfo_uses_the_same_public_anchor_as_other_targets() {
    let mut e = stream();
    e.data(218, &[0; 36]);
    let mut env = vec![0; 68];
    word(&mut env, 44, 64);
    word(&mut env, 48, 32950);
    e.data(218, &env);
    let c = imported(&e);
    let lfo = c.project.channels[0].voice.envelopes.lfos.volume;
    assert!(lfo.enabled);
    assert_eq!(lfo.rate_hz, 0.5);
    assert_eq!(lfo.depth, 0.5);
}

#[test]
fn modern_and_legacy_channel_filter_words_and_recorded_targets_have_independent_units() {
    let mut e = stream();
    let mut levels = vec![0; 24];
    word(&mut levels, 0, 6400);
    word(&mut levels, 4, 10240);
    word(&mut levels, 12, 128);
    word(&mut levels, 16, 128);
    word(&mut levels, 20, 1);
    e.data(219, &levels);
    e.word(65, 1);
    let mut control = Vec::new();
    control.extend(0_u32.to_le_bytes());
    control.extend(2_u32.to_le_bytes());
    control.extend(128_i32.to_le_bytes());
    e.data(223, &control);
    let c = imported(&e);
    let ch = &c.project.channels[0];
    assert_ne!(ch.mixer_track, windfall_project::TrackId::MASTER);
    let track = c.project.mixer.track(ch.mixer_track).unwrap();
    assert_eq!(track.output, Some(windfall_project::TrackId::MASTER));
    let params = track.effects[0].params;
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == "frequencyHz")
        .unwrap();
    assert!((params.get(index).unwrap() - 3048.706).abs() < 0.01);
    let event = &c.project.patterns[0].events[0];
    assert!(
        (c.project
            .automation_range(&event.target)
            .unwrap()
            .value(event.points[0].value)
            - 3048.706)
            .abs()
            < 0.01
    );
    let mut e = stream();
    e.word(71, 512);
    e.word(83, 320);
    let c = imported(&e);
    let ch = &c.project.channels[0];
    let p = c.project.mixer.track(ch.mixer_track).unwrap().effects[0].params;
    let i = p
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == "frequencyHz")
        .unwrap();
    assert!((p.get(i).unwrap() - 1129.1504).abs() < 0.01);
    let i = p
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == "q")
        .unwrap();
    assert_eq!(p.get(i), Some(4.0));
    assert_eq!(ch.expression_map.bend_range_semitones, 2.0);
}

#[test]
fn synthetic_slide_stream_keeps_onset_duration_color_and_original_chord_notes() {
    let mut e = stream();
    e.word(65, 1);
    let mut bytes = Vec::new();
    for (position, length, key, color, slide) in [
        (0_u32, 96_u32, 72_u16, 3_u8, false),
        (12, 72, 60, 3, false),
        (24, 24, 76, 3, true),
        (0, 96, 80, 4, false),
    ] {
        let mut n = vec![0; 24];
        word(&mut n, 0, position as i32);
        word(&mut n, 8, length as i32);
        n[4..6].copy_from_slice(&(if slide { 8_u16 } else { 0 }).to_le_bytes());
        n[12..14].copy_from_slice(&key.to_le_bytes());
        n[16] = 120;
        n[18] = 64;
        n[19] = color;
        n[20] = 64;
        n[21] = 100;
        n[22] = 128;
        n[23] = 128;
        bytes.extend(n);
    }
    e.data(224, &bytes);
    let c = imported(&e);
    let notes = &c.project.patterns[0].lanes[0].notes;
    assert_eq!(notes.len(), 4);
    let slide = notes
        .iter()
        .find(|n| n.expression.articulation == windfall_project::NoteArticulation::Slide)
        .unwrap();
    assert_eq!(
        (
            slide.start,
            slide.length,
            slide.key,
            slide.expression.color_group
        ),
        (240, 240, 76, Some(3))
    );
    assert_eq!(notes.iter().find(|n| n.key == 72).unwrap().length, 960);
    assert!(common::words(&c.report).contains("highest sounding note"));
}

#[test]
fn normalized_plugin_recording_float_words_preserve_zero_midpoint_and_full_scale() {
    let mut e = stream();
    e.word(65, 1);
    let mut records = Vec::new();
    for (tick, value) in [(0_u32, 0.0_f32), (96, 0.5), (192, 1.0)] {
        records.extend(tick.to_le_bytes());
        records.extend(0x20008000_u32.to_le_bytes());
        records.extend(value.to_le_bytes());
    }
    e.data(223, &records);
    e.text(204, "Master");
    e.text(201, "Fruity Fast LP");
    e.data(213, &[0; 12]);
    e.word(98, 0);
    e.dword(147, u32::MAX);
    let c = imported(&e);
    let lane = &c.project.patterns[0].events[0];
    let range = c.project.automation_range(&lane.target).unwrap();
    // (100*(.2+n)^1.5)^2 * 925/2048, n=0,.5,1, bounded to 20..20kHz.
    for (point, want) in lane
        .points
        .iter()
        .zip([36.1328125_f64, 1549.1943359375, 7804.6875].map(|value| value as f32))
    {
        assert!((range.value(point.value) - want).abs() < 0.004);
    }
}

#[test]
fn keyboard_sample_processing_and_named_source_limits_use_synthetic_pcm_evidence() {
    let mut e = stream();
    e.text(196, "sample.wav");
    e.dword(142, 25);
    e.word(80, 50);
    e.word(70, 256);
    let mut p = vec![0; 168];
    p[9] = 1;
    p[11] = 1;
    p[80] = 1;
    p[81] = 1;
    p[68..72].copy_from_slice(&60_u32.to_le_bytes());
    p[72..76].copy_from_slice(&72_u32.to_le_bytes());
    word(&mut p, 84, 128);
    word(&mut p, 88, 64);
    p[136..144].copy_from_slice(&1.0_f64.to_le_bytes());
    e.data(215, &p);
    let mut notes = Vec::new();
    for key in [59_u16, 60, 72, 73] {
        let mut n = [0; 24];
        n[8..12].copy_from_slice(&96_u32.to_le_bytes());
        n[12..14].copy_from_slice(&key.to_le_bytes());
        n[16] = 120;
        n[18] = 64;
        n[20] = 64;
        n[21] = 100;
        n[22] = 128;
        n[23] = 128;
        notes.extend(n);
    }
    e.word(65, 1);
    e.data(224, &notes);
    let mut c = imported(&e);
    assert_eq!(
        c.project.patterns[0].lanes[0]
            .notes
            .iter()
            .map(|n| n.key)
            .collect::<Vec<_>>(),
        [60, 72]
    );
    let channel = c.project.channels[0].id;
    let sample = c.project.channels[0].source.sample().unwrap();
    let ChannelSource::Sampler(settings) = &c.project.channels[0].source else {
        panic!("sampler")
    };
    assert_eq!(settings.tune, 0.75);
    assert_eq!(settings.loop_crossfade, 0.5);
    let kinds: Vec<_> = c
        .project
        .mixer
        .track(c.project.channels[0].mixer_track)
        .unwrap()
        .effects
        .iter()
        .map(|e| e.params.kind())
        .collect();
    assert_eq!(
        kinds,
        [
            windfall_dsp::EffectKind::DcBlock,
            windfall_dsp::EffectKind::StereoMatrix,
            windfall_dsp::EffectKind::Polarity
        ]
    );
    assert!(
        windfall_flp::convert::resolve_normalization(
            &mut c.project,
            &c.normalization,
            &[(sample, 0.5)]
        )
        .is_empty()
    );
    let pcm = [0.0_f32, 0.0, 0.1, -0.1, 0.5, -0.5, 0.3, -0.3, 0.0, 0.0];
    assert!(
        windfall_flp::convert::resolve_sample_trim(
            &mut c.project,
            &c.sample_trim,
            &[(sample, &pcm, 2)]
        )
        .is_empty()
    );
    let ChannelSource::Sampler(settings) = &c.project.channel(channel).unwrap().source else {
        panic!("sampler")
    };
    assert_eq!(settings.gain, 2.0);
    assert_eq!((settings.start, settings.end), (0.4, 0.8));
    c.project.check().unwrap();
    let json = windfall_project::file::to_json(&c.project).unwrap();
    assert_eq!(windfall_project::file::from_json(&json).unwrap(), c.project);
    assert!(common::words(&c.report).contains("inclusive playable key region"));
}

#[test]
fn named_layout_absence_and_malformed_native_lengths_report_each_device() {
    for name in ["Sytrus", "Harmless", "Toxic Biohazard", "Sakura"] {
        let c = instrument(name, &[0; 16]);
        let report = common::words(&c.report);
        assert!(report.contains("SCOPE:") && report.contains(name));
        assert_eq!(c.project.retained_plugins[0].state, [0; 16]);
    }
    for name in [
        "Fruity Squeeze",
        "Maximus",
        "Gross Beat",
        "Fruity Peak Controller",
    ] {
        let c = effect(name, &[0; 16]);
        let report = common::words(&c.report);
        assert!(report.contains("SCOPE:") && report.contains(name));
    }
    let c = instrument("Fruity DX10", &[0; 4]);
    assert!(common::words(&c.report).contains("requires 92 bytes"));
}

#[test]
fn native_waveshaper_tension_and_instant_mode_have_independent_sampled_values() {
    for (mode, want) in [(0_u8, -0.63514894_f32), (2, -1.0)] {
        let mut b = vec![0; 34];
        b[16] = 1;
        word(&mut b, 30, 2);
        for (dx, y, tension, m) in [(0.0_f64, -1.0_f64, 0.0_f32, 0_u8), (1.0, 1.0, 0.5, mode)] {
            b.extend(dx.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.extend(tension.to_le_bytes());
            b.push(m);
            b.extend([0; 3]);
        }
        let c = effect("Fruity Waveshaper", &b);
        // At midpoint: -1 + 2*(exp(1.5)-1)/(exp(3)-1); instant holds -1.
        assert!((effect_value(&c, "curve.16") - want).abs() < 0.000001);
        assert_eq!(effect_value(&c, "curve.32"), 1.0);
        assert!(common::words(&c.report).contains("shape.point1"));
    }
}

#[test]
fn slicex_public_filename_embedded_wave_and_cue_frames_translate_and_reopen() {
    let mut body = b"WAVE".to_vec();
    let mut fmt = Vec::new();
    fmt.extend(1_u16.to_le_bytes());
    fmt.extend(2_u16.to_le_bytes());
    fmt.extend(48000_u32.to_le_bytes());
    fmt.extend(192000_u32.to_le_bytes());
    fmt.extend(4_u16.to_le_bytes());
    fmt.extend(16_u16.to_le_bytes());
    body.extend(b"fmt ");
    body.extend(16_u32.to_le_bytes());
    body.extend(fmt);
    body.extend(b"data");
    body.extend(16_u32.to_le_bytes());
    for value in [0_i16, 0, 16384, 16384, -16384, -16384, 8192, 8192] {
        body.extend(value.to_le_bytes());
    }
    body.extend(b"cue ");
    body.extend(52_u32.to_le_bytes());
    body.extend(2_u32.to_le_bytes());
    for (id, frame) in [(1_u32, 0_u32), (2, 2)] {
        body.extend(id.to_le_bytes());
        body.extend(0_u32.to_le_bytes());
        body.extend(b"data");
        body.extend(0_u64.to_le_bytes());
        body.extend(frame.to_le_bytes());
    }
    let mut wave = b"RIFF".to_vec();
    wave.extend((body.len() as u32).to_le_bytes());
    wave.extend(body);
    let mut state = vec![0; 96];
    word(&mut state, 12, 12800);
    word(&mut state, 24, 300);
    state[89] = 1;
    state.push(0);
    state.extend((wave.len() as u32).to_le_bytes());
    state.extend(wave);
    let c = instrument("Slicex", &state);
    let asset = c.project.channels[0].sample_playback.as_ref().unwrap();
    assert_eq!(asset.inline_source.as_ref().unwrap().sample_rate, 48000);
    assert_eq!(
        asset.inline_source.as_ref().unwrap().data,
        [0.0, 0.5, -0.5, 0.25]
    );
    assert_eq!(asset.slices.len(), 2);
    assert_eq!(
        (
            asset.slices[0].start,
            asset.slices[0].end,
            asset.slices[1].start,
            asset.slices[1].end
        ),
        (0.0, 0.5, 0.5, 1.0)
    );
    assert_eq!(asset.slices[0].pitch, 3.0);
    assert_eq!(c.project.retained_plugins[0].state, state);
    assert!(common::words(&c.report).contains("embedded WAV PCM/cues"));
    assert_eq!(
        windfall_project::file::from_json(&windfall_project::file::to_json(&c.project).unwrap())
            .unwrap(),
        c.project
    );
    let path = b"public-reference.wav";
    let mut state = vec![0; 96];
    state.push(path.len() as u8);
    state.extend(path);
    state.extend(0_u32.to_le_bytes());
    let c = instrument("Slicex", &state);
    assert_eq!(c.sample_cues.len(), 1);
    assert_eq!(
        c.project.samples[0].path,
        windfall_project::SamplePath::External("public-reference.wav".into())
    );
}

#[test]
fn keyboard_add_root_and_source_fields_with_no_units_have_named_reports() {
    let mut e = stream();
    e.dword(135, 72);
    let mut p = vec![0; 168];
    p[63] = 1;
    word(&mut p, 72, 256);
    word(&mut p, 112, 1);
    p[83] = 3;
    p[157] = 1;
    p[20..24].copy_from_slice(&0.25_f32.to_le_bytes());
    e.data(215, &p);
    e.word(65, 1);
    let mut n = vec![0; 24];
    word(&mut n, 8, 96);
    n[12] = 60;
    n[16] = 120;
    n[18] = 64;
    n[20] = 64;
    n[21] = 100;
    n[22] = 128;
    n[23] = 128;
    e.data(224, &n);
    let c = imported(&e);
    assert_eq!(c.project.patterns[0].lanes[0].notes[0].key, 72);
    let report = common::words(&c.report);
    for text in [
        "keyboard add-root",
        "MIDI channel through=1",
        "declick mode=3",
        "fix legacy trim=1",
        "drum stretch tone=0.25",
        "SCOPE:",
    ] {
        assert!(report.contains(text), "{text}: {report}");
    }
}

#[test]
fn dx10_fine_ratio_and_pitch_lfo_use_public_power_and_rate_rules() {
    let mut b = vec![0; 92];
    word(&mut b, 16, 32768);
    word(&mut b, 20, 32768);
    word(&mut b, 24, 32768);
    word(&mut b, 44, 32768);
    word(&mut b, 56, 32768);
    let c = instrument("Fruity DX10", &b);
    // 20 * 2^(11.25/1200), where fine=(.5)^4*180 cents.
    assert!((instrument_value(&c, "operators.1.ratio") - 20.13039).abs() < 0.00001);
    let lfo = c.project.channels[0].voice.envelopes.lfos.pitch;
    assert!(lfo.enabled);
    assert_eq!(lfo.rate_hz, 20.0);
    assert!((lfo.depth - 0.09090909).abs() < 0.000001);
    assert!(common::words(&c.report).contains("fine=(raw/65536)^4*180"));
}

#[test]
fn bassdrum_filename_reference_resolves_to_persisted_pcm_layer_with_measured_samples() {
    let mut payload = vec![0; 1376];
    let name = b"layer.wav";
    payload[260..260 + name.len()].copy_from_slice(name);
    payload[1288..1292].copy_from_slice(&0.5_f32.to_le_bytes());
    let mut state = b"SSLF".to_vec();
    state.extend(1376_u32.to_le_bytes());
    state.extend(payload);
    let mut c = instrument("BassDrum", &state);
    assert_eq!(c.layer_samples.len(), 1);
    assert_eq!(instrument_value(&c, "sampleMix"), 0.5);
    let sample = c.layer_samples[0].1;
    let audio = windfall_flp::convert::resolve_layer_samples(
        &mut c.project,
        &c.layer_samples,
        &[(
            sample,
            windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.0, 0.5, -0.5, 1.0]),
        )],
    );
    assert!(audio.is_empty());
    let ChannelSource::Instrument {
        asset: Some(windfall_dsp::InstrumentAsset::Pcm(ref layer)),
        ..
    } = c.project.channels[0].source
    else {
        panic!("layer PCM")
    };
    assert_eq!(layer.samples, [0, 16384, -16384, 32767]);
    assert_eq!(layer.sample_rate, 48000);
    assert_eq!(
        windfall_project::file::from_json(&windfall_project::file::to_json(&c.project).unwrap())
            .unwrap(),
        c.project
    );
}

#[test]
fn effector_commented_public_float_prefix_maps_xy_mix_and_bypass_without_guessing_models() {
    let mut b = vec![0; 52];
    for (at, v) in [(12, 0.5_f32), (16, 0.25), (20, 0.75)] {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    let c = effect("Effector", &b);
    assert_eq!(effect_value(&c, "mix"), 0.5);
    assert_eq!(effect_value(&c, "x"), 0.25);
    assert_eq!(effect_value(&c, "y"), 0.75);
    assert_eq!(effect_value(&c, "xyEnabled"), 1.0);
    assert!(c.project.mixer.tracks[0].effects[0].enabled);
    b[8..12].copy_from_slice(&1.0_f32.to_le_bytes());
    let c = effect("Effector", &b);
    assert!(!c.project.mixer.tracks[0].effects[0].enabled);
    assert!(common::words(&c.report).contains("effect=0"));
}

#[test]
fn instrument_automation_clip_targets_name_the_device_and_parameter_without_inferred_numbers() {
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, "BooBass");
    e.data(213, &[0; 16]);
    e.word(64, 1);
    e.byte(21, 5);
    let mut curve = vec![0; 21];
    word(&mut curve, 17, 2);
    for (dx, value) in [(0.0_f64, 0.25_f64), (1.0, 0.75)] {
        curve.extend(dx.to_le_bytes());
        curve.extend(value.to_le_bytes());
        curve.extend([0; 8]);
    }
    e.data(234, &curve);
    let mut link = vec![0; 20];
    word(&mut link, 2, 1);
    word(&mut link, 8, 0x8001);
    e.data(227, &link);
    let c = imported(&e);
    assert!(c.project.automations.is_empty());
    let report = common::words(&c.report);
    assert!(
        report.contains("Instrument BooBass parameter 1")
            && report.contains("SCOPE:")
            && report.contains("no named automation numbers"),
        "{report}"
    );
}

#[test]
fn rack_group_names_and_invalid_source_indices_have_specific_reports() {
    for (index, want) in [(0_u32, "Drums"), (9, ""), (u32::MAX, "")] {
        let mut e = stream();
        e.text(231, " Drums ");
        e.dword(145, index);
        let c = imported(&e);
        assert_eq!(c.project.channels[0].group.as_str(), want);
        assert!(common::words(&c.report).contains("source rack group"));
    }
}

#[test]
fn free_filter_public_response_names_use_append_only_target_choices_and_report_range_dispute() {
    for (source, want) in [
        (88, 0.0),
        (175, 2.0),
        (263, 1.0),
        (350, 3.0),
        (438, 4.0),
        (525, 5.0),
        (700, 6.0),
    ] {
        let mut b = vec![0; 21];
        word(&mut b, 4, source);
        word(&mut b, 8, 1024);
        let c = effect("Fruity Free Filter", &b);
        assert_eq!(effect_value(&c, "mode"), want);
        assert!((effect_value(&c, "frequencyHz") - 16000.0).abs() < 0.001);
        assert!(common::words(&c.report).contains("disagree"));
    }
}

#[test]
fn pitch_shifter_named_display_tail_is_decoded_without_inventing_the_raw_256_byte_schema() {
    let mut b = vec![0; 331];
    word(&mut b, 4, 500);
    b[70] = 12;
    b[73] = 1;
    b[74] = 1;
    let c = effect("Pitch Shifter", &b);
    assert_eq!(effect_value(&c, "semitones"), 5.0);
    let report = common::words(&c.report);
    for field in ["display_speed=12", "display_on=1", "display_mono=1"] {
        assert!(report.contains(field));
    }
    assert_eq!(c.project.retained_plugins[0].state, b);
}
