use super::*;

// Parameter construction copies the inline sample tables, so give each test body
// enough stack without increasing the test harness's default thread stack.
fn with_large_stack(test: impl FnOnce() + Send + 'static) {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(test)
        .expect("failed to spawn zone test thread")
        .join();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

const RATE: f32 = 8000.0;
const CENTER: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn reference_pan(law: windfall_core::PanLaw, pan: f32) -> (f32, f32) {
    use windfall_core::PanLaw::*;
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    let power = if pan == 0.0 {
        (1.0, 1.0)
    } else if pan == -1.0 {
        (std::f32::consts::SQRT_2, 0.0)
    } else if pan == 1.0 {
        (0.0, std::f32::consts::SQRT_2)
    } else {
        (
            angle.cos() * std::f32::consts::SQRT_2,
            angle.sin() * std::f32::consts::SQRT_2,
        )
    };
    match law {
        Balance => (1.0 - pan.max(0.0), 1.0 + pan.min(0.0)),
        ConstantPower => power,
        Compromise => (
            (power.0 * (1.0 - pan)).sqrt(),
            (power.1 * (1.0 + pan)).sqrt(),
        ),
        Linear => (1.0 - pan, 1.0 + pan),
    }
}

#[test]
fn sample_bank_project_laws_match_independent_pcm_for_each_note_and_reset() {
    with_large_stack(|| {
        use windfall_core::PanLaw::*;
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: MAX_SAMPLE_FRAMES as u16,
            ..zone(0.8)
        };
        let mut instrument = sampler(&[z]);
        for law in [Balance, ConstantPower, Compromise, Linear] {
            for pan in [-1.0, -0.4, 0.0, 0.65, 1.0] {
                instrument.reset();
                instrument.set_note_pan_law(law);
                instrument.note_on_instance(
                    NoteInstanceId(7),
                    60,
                    0.5,
                    pan,
                    NoteExpression::NEUTRAL,
                );
                let (mut left, mut right) = ([0.0; 100], [0.0; 100]);
                instrument.process(&mut left, &mut right);
                let (l, r) = reference_pan(law, pan);
                for index in 0..100 {
                    assert!(
                        (left[index] - 0.4 * CENTER * l).abs() < 1e-6,
                        "{law:?} {pan} left"
                    );
                    assert!(
                        (right[index] - 0.4 * CENTER * r).abs() < 1e-6,
                        "{law:?} {pan} right"
                    );
                }
                instrument.set_note_expression(NoteInstanceId(7), -pan, NoteExpression::NEUTRAL);
                instrument.note_on_instance(
                    NoteInstanceId(8),
                    60,
                    0.25,
                    pan,
                    NoteExpression::NEUTRAL,
                );
                instrument.process(&mut left, &mut right);
                let (a, b) = reference_pan(law, -pan);
                assert!((left[0] - CENTER * (0.4 * a + 0.2 * l)).abs() < 1e-6);
                assert!((right[0] - CENTER * (0.4 * b + 0.2 * r)).abs() < 1e-6);
                instrument.reset();
                instrument.process(&mut left, &mut right);
                assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
            }
        }
    });
}

#[test]
fn sample_bank_live_law_ramp_is_five_ms_and_independent_of_callback_partition() {
    with_large_stack(|| {
        use windfall_core::PanLaw::*;
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: MAX_SAMPLE_FRAMES as u16,
            ..zone(0.8)
        };
        let pan = 0.65;
        let from = reference_pan(Balance, pan);
        let to = reference_pan(Linear, pan);
        let mut baseline = None;
        for block in [1, 7, 17, 64, 127] {
            let mut instrument = sampler(&[z]);
            instrument.set_note_pan_law(Balance);
            instrument.note_on_instance(NoteInstanceId(1), 60, 0.5, pan, NoteExpression::NEUTRAL);
            render(&mut instrument, 53, block);
            instrument.set_note_pan_law(Linear);
            let (mut left, mut right) = (vec![0.0; 101], vec![0.0; 101]);
            for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
                instrument.process(l, r);
            }
            for index in 0..101 {
                let part = (index as f32 / 40.0).min(1.0);
                let l = from.0 + (to.0 - from.0) * part;
                let r = from.1 + (to.1 - from.1) * part;
                assert!((left[index] - 0.4 * CENTER * l).abs() < 1e-6);
                assert!((right[index] - 0.4 * CENTER * r).abs() < 1e-6);
            }
            let stereo = (left, right);
            if let Some(reference) = &baseline {
                assert_eq!(&stereo, reference);
            } else {
                baseline = Some(stereo);
            }
        }
    });
}
fn zone(value: f32) -> Zone {
    Zone {
        sample: Sample::from_slice(&[value; MAX_SAMPLE_FRAMES], RATE).unwrap(),
        ..Zone::default()
    }
}
fn table(zones: &[Zone]) -> ZoneTable {
    ZoneTable::from_slice(zones).unwrap()
}
fn render<I: Instrument>(instrument: &mut I, frames: usize, block: usize) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        instrument.process(l, r);
    }
    assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
    left
}
fn sampler(zones: &[Zone]) -> ZoneSampler {
    let mut sampler = ZoneSampler::new(&ZoneSamplerParams {
        zones: table(zones),
        level: 1.0,
        ..Default::default()
    });
    sampler.prepare(RATE, 64);
    sampler
}

#[test]
fn key_ranges_select_only_the_right_zone_including_boundaries() {
    with_large_stack(|| {
        let a = Zone {
            key_low: 48,
            key_high: 60,
            ..zone(0.8)
        };
        let b = Zone {
            key_low: 61,
            key_high: 72,
            ..zone(-0.4)
        };
        let mut instrument = sampler(&[a, b]);
        for (key, expected) in [
            (47, 0.0),
            (48, 0.8),
            (60, 0.8),
            (61, -0.4),
            (72, -0.4),
            (73, 0.0),
            (128, 0.0),
        ] {
            instrument.reset();
            instrument.note_on(key, 1.0);
            let audio = render(&mut instrument, 1, 1);
            assert!((audio[0] - expected * CENTER).abs() < 1e-6, "key {key}");
        }
    });
}

#[test]
fn outside_velocity_ranges_and_missing_zones_are_exactly_silent() {
    with_large_stack(|| {
        let mut instrument = sampler(&[Zone {
            velocity_low: 0.25,
            velocity_high: 0.75,
            ..zone(1.0)
        }]);
        for velocity in [0.0, -1.0, 0.24, 0.76, f32::NAN, f32::INFINITY] {
            instrument.reset();
            instrument.note_on(60, velocity);
            assert_eq!(instrument.active_voices(), 0);
            assert!(
                render(&mut instrument, 64, 11)
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        for velocity in [0.25, 0.75] {
            instrument.reset();
            instrument.note_on(60, velocity);
            assert_eq!(instrument.active_voices(), 1);
        }
        let mut empty = sampler(&[]);
        empty.note_on(60, 1.0);
        assert!(
            render(&mut empty, 64, 19)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    });
}

#[test]
fn pad_keys_have_independent_sources_and_no_wraparound() {
    with_large_stack(|| {
        let mut params = PadSamplerParams {
            zones: table(&[zone(0.8), zone(-0.4)]),
            level: 1.0,
            ..Default::default()
        };
        params.zones.zones[0].root_key = 100;
        let mut instrument = PadSampler::new(&params);
        instrument.prepare(RATE, 64);
        assert_eq!(instrument.params().zones.zones[0].root_key, 36);
        for (key, expected) in [
            (35, 0.0),
            (36, 0.8),
            (37, -0.4),
            (38, 0.0),
            (51, 0.0),
            (52, 0.0),
        ] {
            instrument.reset();
            instrument.note_on(key, 1.0);
            assert!((render(&mut instrument, 1, 1)[0] - expected * CENTER).abs() < 1e-6);
        }
        params.zones.len = 32;
        assert_eq!(params.sanitized().zones.len, 16);
        assert_eq!(params.sanitized().zones.zones[15].key_low, 51);
    });
}

#[test]
fn keyboard_stretches_one_zone_and_note_off_finishes_its_release() {
    with_large_stack(|| {
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: 1024,
            ..zone(0.8)
        };
        let mut instrument = KeyBed::new(&KeyBedParams {
            zones: table(&[z, zone(-1.0)]),
            level: 1.0,
            release_ms: 20.0,
            ..Default::default()
        });
        instrument.prepare(RATE, 512);
        assert_eq!(instrument.params().zones.len, 1);
        for key in [0, 60, 127] {
            instrument.reset();
            instrument.note_on(key, 1.0);
            assert!(
                render(&mut instrument, 40, 13)
                    .iter()
                    .all(|sample| *sample > 0.0)
            );
            instrument.note_off(key);
            let release = render(&mut instrument, 200, 7);
            assert!(release[0] > release[100]);
            assert!(release[170..].iter().all(|sample| *sample == 0.0));
            assert_eq!(instrument.active_voices(), 0);
        }
    });
}

#[test]
fn sample_rate_and_root_pitch_drive_real_linear_playback() {
    with_large_stack(|| {
        let sample = Sample::from_slice(&[0.0, 0.2, 0.4, 0.6, 0.8], RATE).unwrap();
        let mut instrument = sampler(&[Zone {
            sample,
            ..Zone::default()
        }]);
        instrument.note_on(72, 1.0);
        let audio = render(&mut instrument, 5, 1);
        for (actual, expected) in audio.iter().zip([0.0, 0.4, 0.8, 0.0, 0.0]) {
            assert!((*actual - expected * CENTER).abs() < 1e-6);
        }
        let sample = Sample {
            sample_rate: RATE * 0.5,
            ..sample
        };
        let mut instrument = sampler(&[Zone {
            sample,
            ..Zone::default()
        }]);
        instrument.note_on(60, 1.0);
        let audio = render(&mut instrument, 10, 3);
        for (actual, expected) in audio
            .iter()
            .take(9)
            .zip([0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8])
        {
            assert!((*actual - expected * CENTER).abs() < 1e-6);
        }
        assert_eq!(instrument.active_voices(), 0);
    });
}

#[test]
fn overlaps_crossfade_on_the_selected_axis() {
    with_large_stack(|| {
        let a = Zone {
            velocity_high: 0.75,
            ..zone(1.0)
        };
        let b = Zone {
            velocity_low: 0.25,
            ..zone(-1.0)
        };
        let mut instrument = sampler(&[a, b]);
        let mut previous = f32::INFINITY;
        for velocity in [0.25, 0.375, 0.5, 0.625, 0.75] {
            instrument.reset();
            instrument.note_on(60, velocity);
            let normalized = render(&mut instrument, 1, 1)[0] / velocity / CENTER;
            assert!(normalized < previous);
            if velocity == 0.5 {
                assert!(normalized.abs() < 1e-6);
            }
            previous = normalized;
        }
        let a = Zone {
            key_low: 48,
            key_high: 72,
            ..zone(1.0)
        };
        let b = Zone {
            key_low: 60,
            key_high: 84,
            ..zone(-1.0)
        };
        let mut instrument = sampler(&[a, b]);
        let mut params = *instrument.params();
        params.crossfade = CrossfadeAxis::Key;
        instrument.set_params(&params);
        for (key, value) in [(60, 1.0), (66, 0.0), (72, -1.0)] {
            instrument.reset();
            instrument.note_on(key, 1.0);
            assert!((render(&mut instrument, 1, 1)[0] - value * CENTER).abs() < 1e-6);
        }
    });
}

#[test]
fn coincident_overlap_edges_use_equal_weights_without_nan() {
    with_large_stack(|| {
        let a = Zone {
            velocity_low: 0.5,
            velocity_high: 1.0,
            ..zone(1.0)
        };
        let b = Zone {
            velocity_low: 0.5,
            velocity_high: 1.0,
            ..zone(-0.5)
        };
        let mut instrument = sampler(&[a, b]);
        instrument.note_on(60, 0.5);
        assert!((render(&mut instrument, 1, 1)[0] - 0.125 * CENTER).abs() < 1e-6);
    });
}

#[test]
fn player_installs_a_bank_then_rejects_zone_edits_even_when_flag_is_cleared() {
    with_large_stack(|| {
        let mut params = ZonePlayerParams {
            zones: table(&[zone(0.8)]),
            level: 1.0,
            ..Default::default()
        };
        let mut instrument = ZonePlayer::new(&params);
        instrument.prepare(RATE, 64);
        params.zones = table(&[zone(-0.4)]);
        params.zones_locked = false;
        params.level = 0.5;
        instrument.set_params(&params);
        assert!(instrument.params().zones_locked);
        assert_eq!(instrument.params().zones.zones[0].sample.data[0], 0.8);
        instrument.note_on(60, 1.0);
        assert!((render(&mut instrument, 1, 1)[0] - 0.4 * CENTER).abs() < 1e-6);
    });
}

#[test]
fn editable_sampler_changes_future_notes_and_preserves_sounding_samples() {
    with_large_stack(|| {
        let mut instrument = sampler(&[zone(0.8)]);
        instrument.note_on(60, 1.0);
        let mut params = *instrument.params();
        params.zones = table(&[zone(-0.4)]);
        instrument.set_params(&params);
        assert!((render(&mut instrument, 1, 1)[0] - 0.8 * CENTER).abs() < 1e-6);
        instrument.note_on(60, 1.0);
        assert!((render(&mut instrument, 1, 1)[0] - 0.4 * CENTER).abs() < 1e-6);
        instrument.reset();
        instrument.note_on(60, 1.0);
        assert!((render(&mut instrument, 1, 1)[0] + 0.4 * CENTER).abs() < 1e-6);
        params.zones_locked = true;
        instrument.set_params(&params);
        params.zones = table(&[zone(1.0)]);
        instrument.set_params(&params);
        assert_eq!(instrument.params().zones.zones[0].sample.data[0], -0.4);
    });
}

#[test]
fn polyphony_is_bounded_and_emergency_stop_retires_every_layer() {
    with_large_stack(|| {
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: 1024,
            ..zone(1.0)
        };
        let mut instrument = sampler(&[z; MAX_ZONES]);
        for id in 0..100 {
            instrument.note_on_instance(
                NoteInstanceId(id),
                60,
                1.0,
                0.0,
                NoteExpression::default(),
            );
            assert_eq!(instrument.active_voices(), MAX_POLYPHONY);
        }
        assert!((render(&mut instrument, 1, 1)[0] - CENTER).abs() < 1e-6);
        instrument.all_notes_off();
        let audio = render(&mut instrument, 64, 9);
        assert!(audio[48..].iter().all(|sample| *sample == 0.0));
        assert_eq!(instrument.active_voices(), 0);
    });
}

#[test]
fn note_instances_release_and_pan_independently() {
    with_large_stack(|| {
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: 1024,
            ..zone(0.8)
        };
        let mut instrument = sampler(&[z]);
        assert!(instrument.supports_note_instances());
        instrument.note_on_instance(NoteInstanceId(1), 60, 1.0, -1.0, NoteExpression::default());
        instrument.note_on_instance(NoteInstanceId(2), 60, 1.0, 1.0, NoteExpression::default());
        instrument.note_off_instance(NoteInstanceId(1), 60);
        render(&mut instrument, 900, 11);
        assert_eq!(instrument.active_voices(), 1);
        let (mut left, mut right) = ([0.0; 1], [0.0; 1]);
        instrument.process(&mut left, &mut right);
        assert_eq!(left, [0.0]);
        assert_eq!(right, [0.8]);
        instrument.set_note_expression(NoteInstanceId(2), -1.0, NoteExpression::default());
        instrument.process(&mut left, &mut right);
        assert_eq!(left, [0.8]);
        assert_eq!(right, [0.0]);
    });
}

#[test]
fn loops_wrap_interpolation_and_until_release_plays_the_remainder() {
    with_large_stack(|| {
        let sample = Sample::from_slice(&[0.1, 0.2, 0.4, 0.8], RATE).unwrap();
        let z = Zone {
            sample,
            loop_mode: LoopMode::UntilRelease,
            loop_start: 1,
            loop_end: 3,
            ..Zone::default()
        };
        let mut instrument = sampler(&[z]);
        instrument.note_on(60, 1.0);
        let audio = render(&mut instrument, 6, 2);
        for (actual, expected) in audio.iter().zip([0.1, 0.2, 0.4, 0.2, 0.4, 0.2]) {
            assert!((*actual - expected * CENTER).abs() < 1e-6);
        }
        instrument.note_off(60);
        let audio = render(&mut instrument, 4, 1);
        assert!(audio[1] > audio[0]);
        assert_eq!(audio[2..], [0.0, 0.0]);
        assert_eq!(instrument.active_voices(), 0);
    });
}

#[test]
fn playback_and_control_smoothing_do_not_depend_on_block_size() {
    with_large_stack(|| {
        let z = Zone {
            loop_mode: LoopMode::Continuous,
            loop_start: 0,
            loop_end: 1024,
            ..zone(0.8)
        };
        let mut a = sampler(&[z]);
        let mut b = sampler(&[z]);
        for instrument in [&mut a, &mut b] {
            instrument.note_on(60, 1.0);
            render(instrument, 1, 1);
            let mut params = *instrument.params();
            params.level = 0.25;
            params.release_ms = 1.0;
            instrument.set_params(&params);
        }
        assert_eq!(render(&mut a, 513, 513), render(&mut b, 513, 7));
        a.note_off(60);
        b.note_off(60);
        assert_eq!(render(&mut a, 513, 31), render(&mut b, 513, 1));
    });
}

#[test]
fn damaged_params_expression_and_extreme_pitch_never_produce_nan() {
    with_large_stack(|| {
        let mut params = ZoneSamplerParams {
            zones: table(&[zone(1.0)]),
            level: f32::NAN,
            release_ms: f32::INFINITY,
            ..Default::default()
        };
        let zone = &mut params.zones.zones[0];
        zone.sample.len = u16::MAX;
        zone.sample.sample_rate = f32::NAN;
        zone.sample.data[0] = f32::NAN;
        zone.sample.data[1] = f32::INFINITY;
        zone.gain = f32::MAX;
        zone.pan = f32::NEG_INFINITY;
        zone.tune_cents = f32::NAN;
        zone.loop_mode = LoopMode::Continuous;
        zone.loop_start = 0;
        zone.loop_end = u16::MAX;
        params.zones.len = u8::MAX;
        let mut instrument = ZoneSampler::new(&params);
        instrument.prepare(f32::NAN, 1);
        let expression = NoteExpression {
            release: f32::NAN,
            fine_pitch_cents: f32::INFINITY,
            ..Default::default()
        };
        for (id, pitch) in [f32::NAN, f32::INFINITY, -f32::MAX, f32::MAX]
            .into_iter()
            .enumerate()
        {
            instrument.note_on_instance(
                NoteInstanceId(id as u64),
                60,
                f32::MAX,
                f32::NAN,
                expression,
            );
            instrument.set_note_pitch(NoteInstanceId(id as u64), pitch);
            render(&mut instrument, 600, 17);
        }
        instrument.all_notes_off();
        render(&mut instrument, 2000, 9);
        assert_eq!(instrument.active_voices(), 0);
    });
}

fn parameter_contract<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned + ts_rs::TS>() {
    fn is_copy<T: Copy>() {}
    is_copy::<P>();
    let default = P::default();
    assert_eq!(default.sanitized(), default);
    assert_eq!(serde_json::from_str::<P>("{}").unwrap(), default);
    assert_eq!(
        serde_json::from_str::<P>(&serde_json::to_string(&default).unwrap()).unwrap(),
        default
    );
    assert!(!P::decl(&ts_rs::Config::default()).is_empty());
    let mut params = default;
    for (index, info) in P::descriptors().iter().enumerate() {
        assert_eq!(default.get(index), Some(info.default));
        assert_eq!(P::index_of(info.id), Some(index));
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            -f32::MAX,
        ] {
            assert!(params.set(index, value));
            let current = params.get(index).unwrap();
            assert!(current.is_finite() && current >= info.min && current <= info.max);
        }
    }
    assert_eq!(params.get(3), None);
    assert!(!params.set(3, 0.0));
}
#[test]
fn copy_parameter_serialization_names_and_descriptors_agree() {
    with_large_stack(|| {
        parameter_contract::<ZoneSamplerParams>();
        parameter_contract::<ZonePlayerParams>();
        parameter_contract::<PadSamplerParams>();
        parameter_contract::<KeyBedParams>();
        assert_eq!(ZoneSamplerParams::NAME, "Zone Sampler");
        assert_eq!(ZonePlayerParams::NAME, "Zone Player");
        assert_eq!(PadSamplerParams::NAME, "Pad Sampler");
        assert_eq!(KeyBedParams::NAME, "Key Bed");
    });
}

#[test]
fn table_construction_and_deserialization_enforce_caps() {
    assert_eq!(
        Sample::from_slice(&[0.0; 1025], RATE),
        Err(ZoneTableError::TooManyFrames { frames: 1025 })
    );
    assert_eq!(
        ZoneTable::from_slice(&[Zone::default(); 33]),
        Err(ZoneTableError::TooManyZones { zones: 33 })
    );
    let sample: Sample = serde_json::from_str(r#"{"data":[0.1,0.2],"len":2}"#).unwrap();
    assert_eq!(sample.data[..3], [0.1, 0.2, 0.0]);
    let value = serde_json::json!({"data": vec![0.0; 1025]});
    assert!(serde_json::from_value::<Sample>(value).is_err());
}

fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = id.to_vec();
    result.extend_from_slice(&(data.len() as u32).to_le_bytes());
    result.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        result.push(0);
    }
    result
}
fn list(id: &[u8; 4], leaves: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut data = id.to_vec();
    for (id, bytes) in leaves {
        data.extend(chunk(id, bytes));
    }
    chunk(b"LIST", &data)
}
fn put16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn gens(entries: &[(u16, u16)]) -> Vec<u8> {
    entries
        .iter()
        .flat_map(|(op, amount)| op.to_le_bytes().into_iter().chain(amount.to_le_bytes()))
        .collect()
}
struct Bank {
    phdr: Vec<u8>,
    pbag: Vec<u8>,
    pgen: Vec<u8>,
    pmod: Vec<u8>,
    inst: Vec<u8>,
    ibag: Vec<u8>,
    igen: Vec<u8>,
    imod: Vec<u8>,
    shdr: Vec<u8>,
    smpl: Vec<u8>,
}
impl Bank {
    fn new(frames: usize) -> Self {
        let mut phdr = vec![0; 76];
        phdr[..4].copy_from_slice(b"Test");
        phdr[38..41].copy_from_slice(b"EOP");
        put16(&mut phdr, 20, 7);
        put16(&mut phdr, 22, 3);
        put16(&mut phdr, 38 + 24, 1);
        let mut inst = vec![0; 44];
        inst[..4].copy_from_slice(b"Tone");
        inst[22..25].copy_from_slice(b"EOI");
        put16(&mut inst, 42, 1);
        let mut shdr = vec![0; 92];
        shdr[..4].copy_from_slice(b"Wave");
        shdr[46..49].copy_from_slice(b"EOS");
        put32(&mut shdr, 24, frames as u32);
        put32(&mut shdr, 28, 1);
        put32(&mut shdr, 32, (frames - 1) as u32);
        put32(&mut shdr, 36, RATE as u32);
        shdr[40] = 60;
        shdr[41] = (-3_i8) as u8;
        put16(&mut shdr, 44, 1);
        let smpl = (0..frames)
            .flat_map(|index| (((index % 200) as i16 + 1) * 100).to_le_bytes())
            .collect();
        Self {
            phdr,
            inst,
            shdr,
            smpl,
            pbag: gens(&[(0, 0), (1, 0)]),
            pgen: gens(&[(41, 0), (0, 0)]),
            pmod: vec![0; 10],
            ibag: gens(&[(0, 0), (4, 0)]),
            igen: gens(&[
                (43, 48 | (72 << 8)),
                (44, 32 | (100 << 8)),
                (54, 3),
                (53, 0),
                (0, 0),
            ]),
            imod: vec![0; 10],
        }
    }
    fn bytes(&self) -> Vec<u8> {
        let mut data = b"sfbk".to_vec();
        data.extend(list(
            b"INFO",
            &[(b"ifil", &[2, 0, 4, 0]), (b"INAM", b"Test\0")],
        ));
        data.extend(list(b"sdta", &[(b"smpl", &self.smpl)]));
        data.extend(list(
            b"pdta",
            &[
                (b"phdr", &self.phdr),
                (b"pbag", &self.pbag),
                (b"pmod", &self.pmod),
                (b"pgen", &self.pgen),
                (b"inst", &self.inst),
                (b"ibag", &self.ibag),
                (b"imod", &self.imod),
                (b"igen", &self.igen),
                (b"shdr", &self.shdr),
            ],
        ));
        chunk(b"RIFF", &data)
    }
}

#[test]
fn minimal_soundfont_loads_real_pcm_ranges_pitch_loops_and_selected_preset() {
    with_large_stack(|| {
        let bytes = Bank::new(8).bytes();
        let zones = parse_soundfont(&bytes).unwrap();
        assert_eq!(parse_soundfont_preset(&bytes, 3, 7).unwrap(), zones);
        assert_eq!(
            parse_soundfont_preset(&bytes, 0, 0),
            Err(SoundFontError::PresetNotFound { bank: 0, preset: 0 })
        );
        assert_eq!(zones.len, 1);
        let zone = zones.zones[0];
        assert_eq!((zone.key_low, zone.key_high, zone.root_key), (48, 72, 60));
        assert_eq!(
            (zone.velocity_low, zone.velocity_high),
            (32.0 / 127.0, 100.0 / 127.0)
        );
        assert_eq!(zone.sample.len, 8);
        assert_eq!(zone.sample.data[0], 100.0 / 32768.0);
        assert_eq!(zone.sample.sample_rate, RATE);
        assert_eq!(zone.tune_cents, -3.0);
        assert_eq!(zone.loop_mode, LoopMode::UntilRelease);
        let mut instrument = sampler(&[zone]);
        instrument.note_on(60, 0.5);
        assert!(
            render(&mut instrument, 40, 11)
                .iter()
                .all(|sample| *sample > 0.0)
        );
    });
}

#[test]
fn every_truncated_soundfont_prefix_returns_an_error() {
    let bytes = Bank::new(8).bytes();
    for end in 0..bytes.len() {
        assert!(parse_soundfont(&bytes[..end]).is_err(), "prefix {end}");
    }
    assert!(parse_soundfont(&bytes).is_ok());
    let mut forged = bytes[..bytes.len() - 1].to_vec();
    let size = forged.len() as u32 - 8;
    put32(&mut forged, 4, size);
    assert_eq!(parse_soundfont(&forged), Err(SoundFontError::Truncated));
}

#[test]
fn soundfont_errors_are_typed_for_caps_invalid_indices_and_unsupported_features() {
    let mut bank = Bank::new(1025);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::TooManyFrames {
            sample: 0,
            frames: 1025
        })
    );
    bank = Bank::new(8);
    put16(&mut bank.igen, 3 * 4 + 2, 1);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::InvalidTable(*b"igen"))
    );
    bank = Bank::new(8);
    put16(&mut bank.shdr, 44, 4);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::UnsupportedSampleType(4))
    );
    bank = Bank::new(8);
    put16(&mut bank.igen, 2 * 4, 8);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::UnsupportedGenerator(8))
    );
    bank = Bank::new(8);
    put32(&mut bank.shdr, 24, 9999);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::InvalidSample(0))
    );
    bank = Bank::new(8);
    bank.imod.extend([0; 10]);
    put16(&mut bank.ibag, 6, 1);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::UnsupportedModulators)
    );
    bank = Bank::new(8);
    bank.igen = gens(&[(51, 121), (53, 0), (0, 0)]);
    bank.ibag = gens(&[(0, 0), (2, 0)]);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::InvalidTable(*b"igen"))
    );
    bank = Bank::new(8);
    bank.pgen = gens(&[(51, 120), (41, 0), (0, 0)]);
    bank.pbag = gens(&[(0, 0), (2, 0)]);
    bank.igen = gens(&[(51, 120), (53, 0), (0, 0)]);
    bank.ibag = gens(&[(0, 0), (2, 0)]);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::UnsupportedTuning(23997))
    );
}

#[test]
fn soundfont_global_and_local_generators_merge_and_preset_ranges_intersect() {
    let mut bank = Bank::new(8);
    bank.pgen = gens(&[
        (43, 60 | (80 << 8)),
        (17, 100),
        (48, 20),
        (51, 1),
        (41, 0),
        (0, 0),
    ]);
    bank.pbag = gens(&[(0, 0), (5, 0)]);
    bank.igen = gens(&[
        (17, 250),
        (48, 40),
        (51, 1),
        (17, (-100_i16) as u16),
        (53, 0),
        (0, 0),
    ]);
    bank.ibag = gens(&[(0, 0), (3, 0), (5, 0)]);
    put16(&mut bank.inst, 42, 2);
    let zones = parse_soundfont(&bank.bytes()).unwrap();
    let zone = zones.zones[0];
    assert_eq!((zone.key_low, zone.key_high), (60, 80));
    assert_eq!(zone.pan, 0.0); // instrument local -100 overrides global +250; preset adds +100
    assert_eq!(zone.tune_cents, 197.0);
    assert!((zone.gain - 10.0_f32.powf(-60.0 / 200.0)).abs() < 1e-6);
}

#[test]
fn soundfont_zone_count_rejects_overflow_without_partial_success() {
    let mut bank = Bank::new(8);
    bank.igen = gens(&[(53, 0); 33]);
    bank.igen.extend(gens(&[(0, 0)]));
    bank.ibag = gens(&(0..=33).map(|index| (index, 0)).collect::<Vec<_>>());
    put16(&mut bank.inst, 42, 33);
    assert_eq!(
        parse_soundfont(&bank.bytes()),
        Err(SoundFontError::TooManyZones)
    );
}

#[test]
fn parser_handles_damaged_sizes_and_indices_without_panicking() {
    let original = Bank::new(8).bytes();
    // Reproducible byte damage exercises chunk bounds, integer/index validation,
    // and leaf parsing; arbitrary metadata changes are permitted to remain valid.
    for position in 0..original.len() {
        let mut damaged = original.clone();
        damaged[position] = 255;
        let _ = parse_soundfont(&damaged);
    }
}
