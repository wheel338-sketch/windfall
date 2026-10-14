//! Independent public-layout streams: literal ids/offsets, not model->writer
//! round trips. Original synthetic data; no third-party projects or presets.
mod common;
use common::Events;
use windfall_flp::{ConvertOptions, FileFormat, Header, import, parse};
use windfall_project::{ArpeggiatorMode, EchoTime, MarkerKind, NoteDivision};
fn bytes(events: &Events) -> Vec<u8> {
    common::file(
        Header {
            format: FileFormat::Project,
            channel_count: 1,
            ppq: 96,
        },
        &events.bytes,
    )
}
fn stream() -> Events {
    let mut e = Events::new(false);
    e.text(199, "9.0.0");
    e.word(66, 120);
    e.word(64, 0);
    e.byte(21, 0);
    e
}
fn u32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn channel_portamento_uses_public_nonlinear_time_at_the_import_tempo() {
    for (raw, expected) in [(0, 0.0), (512, 7.8125), (1024, 125.0), (2048, 2000.0)] {
        let mut e = stream();
        let mut record = u32s(&[4, raw]);
        record.push(3);
        e.data(221, &record);
        let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
        let poly = result.project.channels[0].voice.polyphony;
        assert!(poly.mono_legato);
        assert_eq!(poly.max_voices, 4);
        assert_eq!(poly.portamento_ms, expected);
        assert!(common::words(&result.report).contains("slide/1024"));
        result.project.check().unwrap();
    }
}

#[test]
fn raw_channel_parameter_offsets_map_arp_and_report_unexpressible_fields() {
    for (direction, mode) in [
        (1, ArpeggiatorMode::Up),
        (2, ArpeggiatorMode::Down),
        (3, ArpeggiatorMode::UpDown),
        (5, ArpeggiatorMode::Off),
    ] {
        let mut e = stream();
        let mut record = vec![0; 112];
        for (offset, value) in [
            (40, direction),
            (44, 3),
            (48, 89),
            (52, 1024),
            (56, 24),
            (92, 2),
        ] {
            record[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        record[60] = 1;
        record[64..66].copy_from_slice(&450_u16.to_le_bytes());
        record[10] = 6;
        e.data(215, &record);
        e.word(83, 12);
        let read = parse(&bytes(&e)).unwrap();
        assert_eq!(read.channels[0].resonance, Some(12));
        assert_eq!(read.channels[0].pitch_range, None);
        let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
        let arp = result.project.channels[0].voice.arpeggiator;
        assert_eq!(arp.mode, mode);
        assert_eq!(arp.range_octaves, 3);
        assert_eq!(arp.gate, 0.5);
        assert_eq!(arp.rate, NoteDivision::Sixteenth);
        let report = common::words(&result.report);
        for field in [
            "chord",
            "repeat",
            "slide",
            "gate=450",
            "ping-pong",
            "fat mode",
        ] {
            assert!(report.contains(field), "{field}: {report}");
        }
        result.project.check().unwrap();
    }
}

#[test]
fn echo_record_reads_signed_fields_and_converts_feedback_pitch_and_time() {
    let mut e = stream();
    e.data(209, &u32s(&[6400, -1600, -700, 4, 96]));
    let read = parse(&bytes(&e)).unwrap();
    let raw = read.channels[0].echo.unwrap();
    assert_eq!((raw.pan, raw.pitch_cents), (-1600, -700));
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    let echo = result.project.channels[0].voice.echo;
    assert!(echo.enabled);
    assert_eq!(echo.feedback, 0.5);
    assert_eq!(echo.repeats, 4);
    assert_eq!(echo.pitch_semitones, -7);
    assert_eq!(echo.time, EchoTime::Milliseconds { ms: 250.0 });
    assert!(common::words(&result.report).contains("echo pan=-1600"));
}

#[test]
fn all_five_envelope_tables_and_lfo_tails_retain_their_owner_and_target() {
    let mut e = stream();
    for _ in 0..5 {
        e.data(
            218,
            &u32s(&[
                0, 1, 100, 10000, 100, 20000, 64, 30000, 64, 100, 100, 64, 32950, 1, 0, 0, 0,
            ]),
        );
    }
    let read = parse(&bytes(&e)).unwrap();
    assert_eq!(read.channels[0].envelopes.len(), 5);
    assert_eq!(read.channels[0].envelopes[4].lfo_shape, Some(1));
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    let env = result.project.channels[0].voice.envelopes;
    assert!(env.pan.enabled && env.filter.enabled && env.resonance.enabled && env.pitch.enabled);
    assert_eq!(
        (
            env.pan.depth,
            env.filter.depth,
            env.resonance.depth,
            env.pitch.depth
        ),
        (0.5, 4.0, 0.5, 1200.0)
    );
    for lfo in [
        env.lfos.pan,
        env.lfos.cutoff,
        env.lfos.resonance,
        env.lfos.pitch,
    ] {
        assert!(lfo.enabled);
        assert_eq!(lfo.rate_hz, 0.5);
        assert_eq!(lfo.depth, 0.5);
    }
    let report = common::words(&result.report);
    for target in [
        "pan ADSR",
        "cutoff ADSR",
        "resonance ADSR",
        "pitch ADSR",
        "volume LFO",
    ] {
        assert!(report.contains(target), "{report}");
    }
    result.project.check().unwrap();
}

#[test]
fn truncated_echo_and_optional_envelope_tails_are_bounded_and_reported() {
    let mut e = stream();
    e.data(209, &[0; 19]);
    e.data(218, &[0; 36]);
    let read = parse(&bytes(&e)).unwrap();
    assert!(read.channels[0].echo.is_none());
    assert!(
        read.diagnostics
            .iter()
            .any(|d| windfall_core::english::english(&d.message).contains("echo"))
    );
    assert!(read.channels[0].envelopes[0].lfo_speed.is_none());
    import(&bytes(&e), &ConvertOptions::default())
        .unwrap()
        .project
        .check()
        .unwrap();
}

#[test]
fn balance_state_and_recorded_parameter_events_use_public_numbered_controls() {
    let mut e = stream();
    e.word(65, 1);
    // Location: insert 0, slot 0, source parameter 1 (volume).
    e.data(223, &u32s(&[0, 0x20008001, 128, 48, 0x20008001, 256]));
    e.text(204, "Master");
    e.text(201, "Fruity Balance");
    e.data(213, &u32s(&[-64, 128]));
    e.word(98, 0);
    e.dword(147, u32::MAX);
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    let effect = &result.project.mixer.tracks[0].effects[0];
    let windfall_dsp::EffectParams::Balance(params) = effect.params else {
        panic!("Balance")
    };
    assert_eq!((params.pan, params.gain), (-0.5, 0.5));
    let lane = &result.project.patterns[0].events[0];
    let windfall_project::AutomationTarget::EffectParam { param, .. } = lane.target else {
        panic!("parameter")
    };
    assert_eq!(param, 0); // Append-only Balance descriptor 0 = gain, 1 = pan.
    let range = result.project.automation_range(&lane.target).unwrap();
    assert!((range.value(lane.points[0].value) - 0.5).abs() < 1e-6);
    assert!((range.value(lane.points[1].value) - 1.0).abs() < 1e-6);
    assert!(common::words(&result.report).contains("two-word state"));
    assert_eq!(result.project.retained_plugins[0].state, u32s(&[-64, 128]));
    result.project.check().unwrap();
}

#[test]
fn simsynth_native_state_maps_oscillators_amplitude_and_preserves_original_blob() {
    let mut state = vec![0; 516];
    for osc in 0..3 {
        for (offset, value) in [
            (0, 0.5_f64),
            (1, 0.75),
            (2, 0.25),
            (3, 0.8),
            (4, 0.5),
            (5, 0.5),
            (6, osc as f64 * 0.1),
        ] {
            state[(osc * 7 + offset) * 8..(osc * 7 + offset) * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        state[436 + osc * 16..440 + osc * 16].copy_from_slice(&1_u32.to_le_bytes());
    }
    for (index, value) in [(35, 0.25_f64), (36, 0.5), (37, 0.75), (38, 0.5), (39, 0.6)] {
        state[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, "SimSynth");
    e.data(213, &state);
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    let windfall_project::ChannelSource::Instrument {
        params: windfall_dsp::InstrumentParams::TripleOsc(params),
        ..
    } = result.project.channels[0].source
    else {
        panic!("Three oscillator")
    };
    assert_eq!(
        (
            params.envelope.attack_ms,
            params.envelope.decay_ms,
            params.envelope.sustain,
            params.envelope.release_ms
        ),
        (62.5, 500.0, 0.75, 500.0)
    );
    assert_eq!(
        params.oscillators.map(|o| o.waveform),
        [
            windfall_dsp::Waveform::Triangle,
            windfall_dsp::Waveform::Saw,
            windfall_dsp::Waveform::Square
        ]
    );
    for osc in params.oscillators {
        assert_eq!(
            (osc.semitones, osc.detune_cents, osc.level),
            (12.0, -50.0, 0.8)
        );
    }
    assert_eq!(params.gain, 0.6);
    assert_eq!(result.project.retained_plugins[0].state, state);
    assert!(common::words(&result.report).contains("svf_cut"));
    let saved = windfall_project::file::to_json(&result.project).unwrap();
    let loaded = windfall_project::file::from_json(&saved).unwrap();
    assert_eq!(loaded, result.project);
    result.project.check().unwrap();
}

#[test]
fn marker_loop_skip_and_pause_use_the_next_source_marker_without_invented_extents() {
    for (kind, expected) in [
        (1, MarkerKind::Loop { end: 960 }),
        (2, MarkerKind::Skip { end: 960 }),
        (3, MarkerKind::Pause),
    ] {
        let mut e = stream();
        e.word(99, 0);
        e.dword(148, (kind as u32) << 24);
        e.text(205, "Navigation");
        e.dword(148, 96);
        e.text(205, "Endpoint");
        let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
        let markers = &result.project.playlist.timeline.markers;
        assert_eq!(markers[0].kind, expected);
        assert_eq!(markers[1].tick, 960);
        result.project.check().unwrap();
    }
    let mut e = stream();
    e.word(99, 0);
    e.dword(148, 1 << 24);
    e.text(205, "Unterminated");
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    assert!(result.project.playlist.timeline.markers.is_empty());
    assert!(common::words(&result.report).contains("no positive source endpoint"));
}

#[test]
fn balance_automation_clip_scales_the_public_parameter_span() {
    let mut e = stream();
    e.word(64, 1);
    e.byte(21, 5);
    e.text(203, "Balance curve");
    let mut curve = vec![0; 21];
    curve[17..21].copy_from_slice(&2_u32.to_le_bytes());
    for (offset, value) in [(0.0_f64, 0.0_f64), (1.0, 1.0)] {
        curve.extend(offset.to_le_bytes());
        curve.extend(value.to_le_bytes());
        curve.extend([0; 8]);
    }
    e.data(234, &curve);
    let mut link = vec![0; 20];
    link[2..6].copy_from_slice(&1_u32.to_le_bytes());
    link[8..12].copy_from_slice(&0x20008001_u32.to_le_bytes());
    e.data(227, &link);
    e.text(204, "Master");
    e.text(201, "Fruity Balance");
    e.data(213, &u32s(&[0, 256]));
    e.word(98, 0);
    e.dword(147, u32::MAX);
    // Empty playlist still converts linked curves when an arrangement is read.
    e.word(99, 0);
    let result = import(&bytes(&e), &ConvertOptions::default()).unwrap();
    let automation = &result.project.automations[0];
    let range = result.project.automation_range(&automation.target).unwrap();
    assert_eq!(range.value(automation.points[0].value), 0.0);
    assert!((range.value(automation.points[1].value) - 1.25).abs() < 1e-6);
    result.project.check().unwrap();
}
