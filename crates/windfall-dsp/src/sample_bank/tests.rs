use super::*;
use windfall_core::AudioBuffer;

const RATE: f32 = 48000.0;

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

fn law_pcm(unit: &mut impl Instrument, center: &mut impl Instrument) {
    use windfall_core::PanLaw::*;
    for law in [Balance, ConstantPower, Compromise, Linear] {
        for pan in [-1.0, -0.35, 0.0, 0.65, 1.0] {
            unit.reset();
            center.reset();
            unit.set_note_pan_law(law);
            unit.note_on_instance(NoteInstanceId(1), 60, 0.5, pan, NoteExpression::NEUTRAL);
            center.note_on_instance(NoteInstanceId(1), 60, 0.5, 0.0, NoteExpression::NEUTRAL);
            let baseline = render(center, 512, 17);
            let (mut left, mut right) = ([0.0; 512], [0.0; 512]);
            unit.process(&mut left, &mut right);
            let (l, r) = reference_pan(law, pan);
            assert!(baseline.iter().any(|sample| sample.abs() > 0.01));
            for frame in 0..512 {
                assert!((left[frame] - baseline[frame] * l).abs() < 1e-6);
                assert!((right[frame] - baseline[frame] * r).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn all_six_project_sample_bank_instruments_apply_each_law_to_measured_pcm() {
    macro_rules! check {
        ($kind:ty) => {{
            let bank = Arc::new(SampleBank::new(vec![zone(0.8, SampleZone::default())]).unwrap());
            let mut unit = <$kind>::default();
            let mut center = <$kind>::default();
            unit.prepare(RATE, 512);
            center.prepare(RATE, 512);
            unit.install_bank(bank.clone());
            center.install_bank(bank);
            law_pcm(&mut unit, &mut center);
        }};
    }
    check!(MultiSampler);
    check!(SamplePlayer);
    check!(SoundFont);
    check!(PadBank);
    check!(PianoKeys);
    check!(TextVoice);
}

#[test]
fn project_sample_bank_live_law_pcm_ramps_for_five_ms_at_every_callback_partition() {
    use windfall_core::PanLaw::*;
    let pan = 0.65;
    let from = reference_pan(Balance, pan);
    let to = reference_pan(Linear, pan);
    let mut baseline_stereo = None;
    for block in [1, 17, 64, 127, 512] {
        let mut unit = sampler(vec![zone(0.8, SampleZone::default())]);
        let mut center = sampler(vec![zone(0.8, SampleZone::default())]);
        unit.set_note_pan_law(Balance);
        unit.note_on_instance(NoteInstanceId(1), 60, 0.5, pan, NoteExpression::NEUTRAL);
        center.note_on_instance(NoteInstanceId(1), 60, 0.5, 0.0, NoteExpression::NEUTRAL);
        render(&mut unit, 97, block);
        render(&mut center, 97, block);
        unit.set_note_pan_law(Linear);
        let reference = render(&mut center, 512, block);
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
            unit.process(l, r);
        }
        for frame in 0..512 {
            let part = (frame as f32 / 240.0).min(1.0);
            assert!(
                (left[frame] - reference[frame] * (from.0 + (to.0 - from.0) * part)).abs() < 1e-6
            );
            assert!(
                (right[frame] - reference[frame] * (from.1 + (to.1 - from.1) * part)).abs() < 1e-6
            );
        }
        let stereo = (left, right);
        if let Some(reference) = &baseline_stereo {
            assert_eq!(&stereo, reference);
        } else {
            baseline_stereo = Some(stereo);
        }
    }
}
fn zone(value: f32, mapping: SampleZone) -> LoadedZone {
    LoadedZone {
        mapping,
        audio: AudioBuffer::from_interleaved(48000, 1, vec![value; 4096]),
    }
}
fn sampler(zones: Vec<LoadedZone>) -> MultiSampler {
    let mut unit = MultiSampler::default();
    unit.prepare(RATE, 64);
    unit.install_bank(Arc::new(SampleBank::new(zones).unwrap()));
    unit
}
fn render(unit: &mut impl Instrument, frames: usize, block: usize) -> Vec<f32> {
    let mut pcm = vec![0.0; frames];
    let mut right = vec![0.0; block];
    for chunk in pcm.chunks_mut(block) {
        unit.process(chunk, &mut right[..chunk.len()]);
    }
    pcm
}
#[test]
fn inclusive_key_velocity_edges_and_independent_fades() {
    let map = SampleZone {
        key_low: 48,
        key_high: 60,
        velocity_low: 32,
        velocity_high: 96,
        key_fade: 4,
        velocity_fade: 8,
        ..Default::default()
    };
    assert_eq!(map.weight(47, 64), 0.0);
    assert_eq!(map.weight(61, 64), 0.0);
    assert_eq!(map.weight(54, 31), 0.0);
    assert_eq!(map.weight(54, 97), 0.0);
    assert_eq!(map.weight(48, 32), 1.0 / 32.0);
    assert_eq!(map.weight(60, 96), 1.0 / 32.0);
    assert_eq!(map.weight(54, 64), 1.0);
}
#[test]
fn empty_bank_and_retired_note_controls_are_safe_and_silent() {
    let mut unit = sampler(Vec::new());
    // Idle slots have the default id 0; expression callbacks must not look up
    // their default zone index in an empty bank.
    unit.set_note_pitch(NoteInstanceId(0), 72.0);
    unit.set_note_expression(NoteInstanceId(0), 1.0, NoteExpression::default());
    unit.set_note_gain(NoteInstanceId(0), 2.0);
    unit.note_on_instance(NoteInstanceId(0), 60, 1.0, 0.0, NoteExpression::default());
    unit.note_off_instance(NoteInstanceId(0), 60);
    unit.all_notes_off();
    assert_eq!(unit.active_voices(), 0);
    assert_eq!(unit.missed_notes(), 1);
    assert!(render(&mut unit, 64, 7).iter().all(|x| *x == 0.0));

    unit.install_bank(Arc::new(
        SampleBank::new(vec![zone(0.3, SampleZone::default())]).unwrap(),
    ));
    unit.note_on_instance(NoteInstanceId(0), 60, 1.0, 0.0, NoteExpression::default());
    render(&mut unit, 64, 64);
    unit.all_notes_off();
    render(&mut unit, 512, 7);
    unit.install_bank(Arc::new(SampleBank::new(Vec::new()).unwrap()));
    unit.set_note_pitch(NoteInstanceId(0), 72.0);
    unit.set_note_expression(NoteInstanceId(0), -1.0, NoteExpression::default());
    assert!(render(&mut unit, 64, 7).iter().all(|x| *x == 0.0));
}
#[test]
fn overlapping_layers_add_and_missing_keys_are_silent() {
    let map = SampleZone {
        key_low: 60,
        key_high: 60,
        ..Default::default()
    };
    let mut unit = sampler(vec![zone(0.2, map), zone(0.4, map)]);
    unit.note_on(59, 1.0);
    assert_eq!(unit.active_voices(), 0);
    assert!(render(&mut unit, 64, 7).iter().all(|x| *x == 0.0));
    assert_eq!(unit.missed_notes(), 1);
    unit.note_on(60, 1.0);
    assert_eq!(unit.active_voices(), 2);
    let pcm = render(&mut unit, 64, 7);
    assert!((pcm[63] - 0.3).abs() < 1e-5);
}
#[test]
fn round_robin_advances_only_matching_groups_and_chokes_all_old_layers() {
    let mut unit = sampler(vec![
        zone(
            0.2,
            SampleZone {
                key_low: 36,
                key_high: 36,
                root_key: 36,
                round_robin: 1,
                choke_group: 3,
                ..Default::default()
            },
        ),
        zone(
            0.4,
            SampleZone {
                key_low: 36,
                key_high: 36,
                root_key: 36,
                round_robin: 1,
                choke_group: 3,
                ..Default::default()
            },
        ),
        zone(
            0.6,
            SampleZone {
                key_low: 37,
                key_high: 37,
                root_key: 37,
                choke_group: 3,
                ..Default::default()
            },
        ),
    ]);
    unit.note_on(36, 1.0);
    let a = render(&mut unit, 64, 64)[63];
    unit.note_on(35, 1.0);
    unit.note_on(36, 1.0);
    let b = render(&mut unit, 64, 7)[63];
    assert!((b - 2.0 * a).abs() < 1e-5);
    assert_eq!(unit.active_voices(), 1);
    unit.note_on(37, 1.0);
    assert_eq!(unit.active_voices(), 1);
    assert!((render(&mut unit, 64, 1)[63] - 0.3).abs() < 1e-5);
}
#[test]
fn instances_release_independently_and_stop_retires_bounded_voices() {
    let mut unit = sampler(vec![zone(0.3, SampleZone::default())]);
    for id in 0..200 {
        unit.note_on_instance(NoteInstanceId(id), 60, 1.0, 0.0, NoteExpression::default());
    }
    assert_eq!(unit.active_voices(), BANK_VOICES);
    assert_eq!(unit.stolen_layers(), 72);
    render(&mut unit, 64, 64);
    unit.all_notes_off();
    render(&mut unit, 512, 17);
    assert_eq!(unit.active_voices(), 0);
}
#[test]
fn live_style_partitions_match_offline_pcm_with_loops_and_envelopes() {
    let map = SampleZone {
        loop_mode: BankLoop::UntilRelease,
        loop_start: 7,
        loop_end: 43,
        attack_ms: 4.0,
        hold_ms: 2.0,
        decay_ms: 8.0,
        sustain: 0.4,
        ..Default::default()
    };
    let bank = Arc::new(SampleBank::new(vec![zone(0.3, map)]).unwrap());
    let play = |block| {
        let mut unit = MultiSampler::default();
        unit.prepare(RATE, 128);
        unit.install_bank(bank.clone());
        unit.note_on(60, 1.0);
        let mut pcm = render(&mut unit, 4096, block);
        unit.note_off(60);
        pcm.extend(render(&mut unit, 6000, block));
        pcm
    };
    assert_eq!(play(1), play(127));
    assert_eq!(play(7), play(64));
}
#[test]
fn invalid_pcm_mapping_and_loops_fail_before_publication() {
    assert!(SampleBank::new(vec![zone(f32::NAN, SampleZone::default())]).is_err());
    assert!(
        SampleBank::new(vec![zone(
            0.0,
            SampleZone {
                key_low: 100,
                key_high: 2,
                ..Default::default()
            }
        )])
        .is_err()
    );
    assert!(
        SampleBank::new(vec![zone(
            0.0,
            SampleZone {
                loop_mode: BankLoop::Continuous,
                loop_end: 5000,
                ..Default::default()
            }
        )])
        .is_err()
    );
}
#[test]
fn speech_render_is_deterministic_bounded_and_cancellable() {
    let a = sources::render_text("Hello world!", 48.0, 2.0, &mut || true).unwrap();
    let b = sources::render_text("Hello world!", 48.0, 2.0, &mut || true).unwrap();
    assert_eq!(a.samples(), b.samples());
    assert_eq!(a.frames(), 48000);
    assert!(a.samples().iter().any(|x| x.abs() > 0.01));
    assert_eq!(
        sources::render_text("", 48.0, 2.0, &mut || true).unwrap_err(),
        sources::SpeechError::Empty
    );
    assert_eq!(
        sources::render_text("hello", f32::NAN, 2.0, &mut || true).unwrap_err(),
        sources::SpeechError::InvalidControls
    );
    assert_eq!(
        sources::render_text("hello", 48.0, 2.0, &mut || false).unwrap_err(),
        sources::SpeechError::Cancelled
    );
    assert!(matches!(
        sources::render_text("你好", 48.0, 2.0, &mut || true),
        Err(sources::SpeechError::UnsupportedCharacter(_))
    ));
}
#[test]
fn generated_piano_has_multiple_keys_and_velocity_sources() {
    let bank = sources::piano_bank(false);
    assert_eq!(bank.zones().len(), 30);
    assert!(bank.zones().iter().all(|z| z.audio.frames() == 72000));
    assert_ne!(
        bank.zones()[0].audio.samples(),
        bank.zones()[1].audio.samples()
    );
    for key in 0..=127 {
        for velocity in [1, 63, 64, 127] {
            assert_eq!(
                bank.zones()
                    .iter()
                    .filter(|z| z.mapping.weight(key, velocity) > 0.0)
                    .count(),
                1
            );
        }
    }
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
    sm24: Option<Vec<u8>>,
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
            sm24: None,
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
        let mut samples = vec![(b"smpl", self.smpl.as_slice())];
        if let Some(low) = &self.sm24 {
            samples.push((b"sm24", low));
        }
        data.extend(list(b"sdta", &samples));
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
fn sf2_real_pcm_and_preset_ranges_are_not_limited_to_1024_frames() {
    let bytes = Bank::new(8192).bytes();
    let presets = soundfont::presets(&bytes).unwrap();
    assert_eq!((presets[0].bank, presets[0].program), (3, 7));
    let bank = soundfont::parse_soundfont_preset(&bytes, 3, 7).unwrap();
    let z = &bank.zones()[0];
    assert_eq!(z.audio.frames(), 8192);
    assert_eq!((z.mapping.key_low, z.mapping.key_high), (48, 72));
    assert_eq!((z.mapping.velocity_low, z.mapping.velocity_high), (32, 100));
    assert_eq!(z.mapping.loop_mode, BankLoop::UntilRelease);
    assert_eq!(z.mapping.tune_cents, -3.0);
    assert_eq!(z.audio.samples()[0], 100.0 / 32768.0);
}
#[test]
fn sf2_every_truncated_prefix_and_forged_link_fails_without_panicking() {
    let bytes = Bank::new(64).bytes();
    for end in 0..bytes.len() {
        assert!(soundfont::parse_soundfont(&bytes[..end]).is_err(), "{end}");
    }
    let mut forged = Bank::new(64);
    put16(&mut forged.pgen, 2, 999);
    assert!(soundfont::parse_soundfont(&forged.bytes()).is_err());
    for position in (0..bytes.len()).step_by(17) {
        let mut damaged = bytes.clone();
        damaged[position] = 255;
        let _ = soundfont::parse_soundfont(&damaged);
    }
}
#[test]
fn sf2_envelopes_tuning_and_preset_offsets_reach_resolved_zone() {
    let mut fixture = Bank::new(2048);
    fixture.igen = gens(&[
        (43, 48 | (72 << 8)),
        (34, (-1200i16) as u16),
        (36, 0),
        (37, 600),
        (38, 0),
        (51, 1),
        (53, 0),
        (0, 0),
    ]);
    put16(&mut fixture.ibag, 4, 7);
    fixture.pgen = gens(&[(51, 1), (41, 0), (0, 0)]);
    put16(&mut fixture.pbag, 4, 2);
    let bank = soundfont::parse_soundfont(&fixture.bytes()).unwrap();
    let z = bank.zones()[0].mapping;
    assert!((z.attack_ms - 500.0).abs() < 0.01);
    assert!((z.decay_ms - 1000.0).abs() < 0.01);
    assert!((z.sustain - 0.001).abs() < 1e-6);
    assert_eq!(z.tune_cents, 197.0);
}

#[test]
fn sampled_pitch_matches_independent_frequency_measurement_at_multiple_rates() {
    // A mathematical 440 Hz source at root 60 must become 880 Hz at key 72.
    // Count rising zero crossings rather than consulting the resampler state.
    let pcm = (0..48000)
        .map(|frame| (std::f32::consts::TAU * 440.0 * frame as f32 / 48000.0).sin() * 0.25)
        .collect();
    let bank = Arc::new(
        SampleBank::new(vec![LoadedZone {
            mapping: SampleZone::default(),
            audio: AudioBuffer::from_interleaved(48000, 1, pcm),
        }])
        .unwrap(),
    );
    for rate in [24000, 44100, 48000, 96000] {
        let mut unit = MultiSampler::default();
        unit.prepare(rate as f32, 127);
        unit.install_bank(bank.clone());
        unit.note_on(72, 1.0);
        let pcm = render(&mut unit, rate / 4, 127);
        let crossings = pcm
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert!(
            (crossings as i32 - 220).abs() <= 1,
            "{rate} Hz output yielded {crossings} crossings"
        );
    }
}

#[test]
fn sf2_selects_only_the_requested_preset_and_resolves_global_instrument_zones() {
    let mut fixture = Bank::new(256);
    fixture.phdr = vec![0; 114];
    fixture.phdr[..4].copy_from_slice(b"Keys");
    fixture.phdr[38..43].copy_from_slice(b"Drums");
    fixture.phdr[76..79].copy_from_slice(b"EOP");
    put16(&mut fixture.phdr, 20, 7);
    put16(&mut fixture.phdr, 22, 3);
    put16(&mut fixture.phdr, 38 + 20, 8);
    put16(&mut fixture.phdr, 38 + 22, 3);
    put16(&mut fixture.phdr, 38 + 24, 2);
    put16(&mut fixture.phdr, 76 + 24, 3);
    fixture.pbag = gens(&[(0, 0), (1, 0), (3, 0), (5, 0)]);
    fixture.pgen = gens(&[
        (43, 48 | (63 << 8)),
        (44, 10 | (90 << 8)),
        (41, 0),
        (43, 60 | (72 << 8)),
        (41, 1),
        (0, 0),
    ]);
    fixture.inst = vec![0; 66];
    fixture.inst[..4].copy_from_slice(b"Tone");
    fixture.inst[22..26].copy_from_slice(b"Drum");
    fixture.inst[44..47].copy_from_slice(b"EOI");
    put16(&mut fixture.inst, 22 + 20, 2);
    put16(&mut fixture.inst, 44 + 20, 3);
    fixture.ibag = gens(&[(0, 0), (1, 0), (4, 0), (7, 0)]);
    fixture.igen = gens(&[
        (58, 55),
        (43, 55 | (72 << 8)),
        (44, 64 | (100 << 8)),
        (53, 0),
        (43, 62 | (67 << 8)),
        (58, 65),
        (53, 1),
        (0, 0),
    ]);
    fixture.shdr.resize(138, 0);
    fixture.shdr[46..92].fill(0);
    fixture.shdr[46..50].copy_from_slice(b"Drum");
    fixture.shdr[92..95].copy_from_slice(b"EOS");
    put32(&mut fixture.shdr, 46 + 20, 256);
    put32(&mut fixture.shdr, 46 + 24, 512);
    put32(&mut fixture.shdr, 46 + 28, 257);
    put32(&mut fixture.shdr, 46 + 32, 511);
    put32(&mut fixture.shdr, 46 + 36, 48000);
    fixture.shdr[46 + 40] = 64;
    put16(&mut fixture.shdr, 46 + 44, 1);
    fixture
        .smpl
        .extend((0..256).flat_map(|_| (-1000i16).to_le_bytes()));
    let bytes = fixture.bytes();
    let keys = soundfont::parse_soundfont_preset(&bytes, 3, 7).unwrap();
    let drums = soundfont::parse_soundfont_preset(&bytes, 3, 8).unwrap();
    assert_eq!(soundfont::presets(&bytes).unwrap().len(), 2);
    assert_eq!(keys.zones().len(), 1);
    assert_eq!(drums.zones().len(), 1);
    let key = keys.zones()[0].mapping;
    assert_eq!(
        (
            key.key_low,
            key.key_high,
            key.velocity_low,
            key.velocity_high,
            key.root_key
        ),
        (55, 63, 64, 90, 55)
    );
    let drum = drums.zones()[0].mapping;
    assert_eq!((drum.key_low, drum.key_high, drum.root_key), (62, 67, 65));
    assert!(keys.zones()[0].audio.samples()[0] > 0.0);
    assert_eq!(drums.zones()[0].audio.samples()[0], -1000.0 / 32768.0);
    assert!(matches!(
        soundfont::parse_soundfont_preset(&bytes, 3, 9),
        Err(soundfont::SoundFontError::PresetNotFound { .. })
    ));
}

#[test]
fn choke_crossfade_bounds_discontinuity_and_refuses_when_tail_reserve_is_full() {
    let map = SampleZone {
        choke_group: 1,
        ..Default::default()
    };
    let mut unit = sampler(vec![zone(0.8, map)]);
    unit.note_on(60, 1.0);
    let before = render(&mut unit, 256, 64)[255];
    unit.note_on(60, 0.5);
    let pcm = render(&mut unit, 128, 7);
    assert!(
        (pcm[0] - before).abs() < 0.02,
        "choke boundary is a fade, not a cut"
    );
    assert!(pcm.windows(2).all(|x| (x[1] - x[0]).abs() < 0.02));
    assert!((pcm[127] - 0.2).abs() < 1e-5);
    assert!(unit.engine.choked.iter().all(|v| !v.active));
    for _ in 0..BANK_VOICES {
        unit.note_on(60, 1.0);
    }
    let missed = unit.missed_notes();
    unit.note_on(60, 1.0);
    assert_eq!(unit.missed_notes(), missed + 1);
    assert_eq!(
        unit.active_voices(),
        1,
        "refusal preserves the previous strike"
    );
}

#[test]
fn bank_generations_preserve_held_and_releasing_pcm_and_retire_only_after_finish() {
    let map = SampleZone {
        loop_mode: BankLoop::Continuous,
        loop_start: 0,
        loop_end: 4096,
        ..Default::default()
    };
    let mut old = sampler(vec![zone(0.4, map)]);
    let mut reference = sampler(vec![zone(0.4, map)]);
    old.note_on_instance(NoteInstanceId(7), 60, 1.0, 0.0, NoteExpression::default());
    reference.note_on_instance(NoteInstanceId(7), 60, 1.0, 0.0, NoteExpression::default());
    assert_eq!(render(&mut old, 257, 7), render(&mut reference, 257, 64));
    let weak = Arc::downgrade(old.engine.banks[old.engine.current_bank].as_ref().unwrap());
    let mut incoming = sampler(vec![zone(-0.6, map)]);
    incoming.inherit_bank_state(&mut old);
    drop(old);
    assert_eq!(
        render(&mut incoming, 1000, 11),
        render(&mut reference, 1000, 127)
    );
    incoming.note_off_instance(NoteInstanceId(7), 60);
    reference.note_off_instance(NoteInstanceId(7), 60);
    let mut third = sampler(vec![]);
    third.inherit_bank_state(&mut incoming);
    drop(incoming);
    assert!(weak.upgrade().is_some());
    assert_eq!(
        render(&mut third, 6000, 3),
        render(&mut reference, 6000, 64)
    );
    let mut retired = Vec::new();
    for _ in 0..BANK_VOICES * 2 + 1 {
        if let Some(bank) = third.take_unused_bank() {
            retired.push(bank);
        }
    }
    assert!(weak.upgrade().is_some(), "retirement queue owns old PCM");
    drop(retired);
    assert!(
        weak.upgrade().is_none(),
        "control-side queue drain releases finished bank"
    );
}

#[test]
fn bank_budget_counts_pending_and_shared_retained_owners_until_the_last_drop() {
    let budget = Arc::new(BankBudget::new(64));
    let pending = budget.reserve(32).unwrap();
    let mut bank = SampleBank::new(vec![LoadedZone {
        mapping: SampleZone::default(),
        audio: AudioBuffer::from_interleaved(48000, 1, vec![0.0; 8]),
    }])
    .unwrap();
    bank.retain_reservation(pending);
    let held = Arc::new(bank);
    let old_plan = held.clone();
    let next_job = budget.reserve(32).unwrap();
    let refusal = windfall_core::english::english(&budget.reserve(1).unwrap_err());
    assert!(refusal.contains("64 budget"));
    assert!(refusal.contains("64 retained or preparing"));
    drop(held);
    assert_eq!(budget.used(), 64);
    drop(next_job);
    assert_eq!(budget.used(), 32);
    drop(old_plan);
    assert_eq!(budget.used(), 0);
}

#[test]
fn concurrent_bank_jobs_cannot_overbook_the_shared_budget() {
    let budget = Arc::new(BankBudget::new(64));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let release = Arc::new(std::sync::Barrier::new(3));
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let budget = budget.clone();
            let barrier = barrier.clone();
            let release = release.clone();
            std::thread::spawn(move || {
                let lease = budget.reserve(32).unwrap();
                barrier.wait();
                release.wait();
                drop(lease);
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(budget.used(), 64);
    assert!(budget.reserve(1).is_err());
    release.wait();
    for job in jobs {
        job.join().unwrap();
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn zone_modulation_envelope_measures_pitch_filter_and_amplitude_routes() {
    let audio = AudioBuffer::from_interleaved(
        48000,
        1,
        (0..48000)
            .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin() * 0.4)
            .collect(),
    );
    let map = SampleZone {
        loop_mode: BankLoop::Continuous,
        loop_start: 0,
        loop_end: 48000,
        modulation: ZoneModulation {
            decay_ms: 0.0,
            sustain: 1.0,
            envelope_pitch_cents: 1200.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pitched = sampler(vec![LoadedZone {
        mapping: map,
        audio,
    }]);
    pitched.note_on(60, 1.0);
    let pcm = render(&mut pitched, 48000, 17);
    let crossings = pcm.windows(2).filter(|x| x[0] <= 0.0 && x[1] > 0.0).count();
    assert!(
        (crossings as i32 - 880).abs() <= 1,
        "octave envelope: {crossings} crossings/sec"
    );
    let mut amplitude = sampler(vec![zone(
        0.4,
        SampleZone {
            modulation: ZoneModulation {
                envelope_amplitude: -1.0,
                decay_ms: 50.0,
                ..Default::default()
            },
            ..Default::default()
        },
    )]);
    amplitude.note_on(60, 1.0);
    let pcm = render(&mut amplitude, 3000, 7);
    assert_eq!(pcm[0], 0.0);
    assert!((pcm[2999] - 0.2).abs() < 1e-5);
    let play = |cutoff| {
        let audio = AudioBuffer::from_interleaved(
            48000,
            1,
            (0..48000)
                .map(|i| (std::f32::consts::TAU * 4000.0 * i as f32 / RATE).sin() * 0.4)
                .collect(),
        );
        let mut unit = sampler(vec![LoadedZone {
            audio,
            mapping: SampleZone {
                modulation: ZoneModulation {
                    decay_ms: 0.0,
                    sustain: 1.0,
                    envelope_filter_cents: cutoff,
                    ..Default::default()
                },
                ..Default::default()
            },
        }]);
        unit.note_on(60, 1.0);
        render(&mut unit, 12000, 127)
    };
    let power =
        |pcm: &[f32]| pcm[2000..].iter().map(|x| x * x).sum::<f32>() / (pcm.len() - 2000) as f32;
    assert!(
        power(&play(-9600.0)) < power(&play(0.0)) * 0.001,
        "filter envelope attenuates a measured 4 kHz tone"
    );
}

#[test]
fn grouped_lfos_share_routes_and_delay_while_partitioning_remains_exact() {
    let modulation = ZoneModulation {
        lfos: [
            ZoneLfo {
                rate_hz: 5.0,
                delay_ms: 10.0,
                amplitude: 0.8,
                ..Default::default()
            },
            ZoneLfo {
                rate_hz: 3.0,
                pitch_cents: 50.0,
                filter_cents: 1200.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let map = SampleZone {
        modulation_group: 4,
        modulation,
        loop_mode: BankLoop::Continuous,
        loop_start: 0,
        loop_end: 4096,
        ..Default::default()
    };
    let bank = Arc::new(
        SampleBank::new(vec![
            zone(0.2, map),
            zone(
                0.2,
                SampleZone {
                    modulation: ZoneModulation::default(),
                    ..map
                },
            ),
        ])
        .unwrap(),
    );
    assert_eq!(bank.zones()[1].mapping.modulation, modulation);
    let play = |block| {
        let mut unit = MultiSampler::default();
        unit.prepare(RATE, 128);
        unit.install_bank(bank.clone());
        unit.note_on(60, 1.0);
        render(&mut unit, 12000, block)
    };
    let pcm = play(1);
    assert_eq!(pcm, play(127));
    let first = pcm[400];
    let high = pcm[2400];
    let low = pcm[7200];
    assert!(
        (first - 0.2).abs() < 1e-4,
        "amplitude LFO waits for its delay"
    );
    assert!(high > low * 2.0, "LFO modulates measured amplitude");
}

#[test]
fn multisampler_internal_effects_have_measured_echoes_chorus_and_reverb_tails() {
    let impulse = || LoadedZone {
        mapping: SampleZone {
            release_ms: 1.0,
            ..Default::default()
        },
        audio: AudioBuffer::from_interleaved(48000, 1, {
            let mut pcm = vec![0.0; 480];
            pcm[0] = 0.5;
            pcm
        }),
    };
    let play = |kind: u8, block| {
        let mut unit = sampler(vec![impulse()]);
        let mut params = MultiSamplerParams::default();
        match kind {
            1 => {
                params.delay.sync = false;
                params.delay.time_ms = 20.0;
                params.delay.feedback = 0.5;
                params.delay.mix = 1.0;
            }
            2 => params.chorus.mix = 1.0,
            3 => params.reverb.mix = 1.0,
            _ => {}
        }
        unit.set_params(&params);
        unit.reset();
        unit.note_on(60, 1.0);
        render(&mut unit, 24000, block)
    };
    let dry = play(0, 64);
    assert!(dry[1000..].iter().all(|x| *x == 0.0));
    let delay = play(1, 7);
    assert_eq!(delay, play(1, 127));
    assert!(
        delay[900..1100].iter().any(|x| x.abs() > 0.01),
        "20 ms echo"
    );
    assert!(
        delay[1800..2100].iter().any(|x| x.abs() > 0.001),
        "feedback second repeat"
    );
    let chorus = play(2, 7);
    assert_eq!(chorus, play(2, 127));
    assert!(chorus[480..2000].iter().any(|x| x.abs() > 0.01));
    let reverb = play(3, 7);
    assert_eq!(reverb, play(3, 127));
    assert!(reverb[3000..12000].iter().any(|x| x.abs() > 0.0001));
}

#[test]
fn bank_bytes_charge_unused_pcm_capacity_and_workspace_shrink_is_checked() {
    let mut pcm = Vec::with_capacity(32);
    pcm.resize(8, 0.1);
    let audio = AudioBuffer::from_interleaved(48000, 1, pcm);
    assert_eq!(audio.retained_pcm_bytes(), 128);
    assert_eq!(audio.clone().retained_pcm_bytes(), 128);
    let bank = SampleBank::new(vec![LoadedZone {
        mapping: SampleZone::default(),
        audio,
    }])
    .unwrap();
    assert_eq!(bank.bytes(), 128);
    let budget = Arc::new(BankBudget::new(256));
    let mut workspace = budget.reserve(256).unwrap();
    assert!(
        workspace
            .shrink(257)
            .unwrap_err()
            .contains("memory refused")
    );
    assert_eq!(budget.used(), 256);
    workspace.shrink(bank.bytes()).unwrap();
    assert_eq!(budget.used(), 128);
    let second = budget.reserve(128).unwrap();
    assert!(budget.reserve(1).is_err());
    drop(workspace);
    assert_eq!(budget.used(), 128);
    drop(second);
    assert_eq!(budget.used(), 0);
}

#[test]
fn choke_tail_is_reported_after_the_new_strike_has_finished() {
    let long = zone(
        0.4,
        SampleZone {
            key_low: 60,
            key_high: 60,
            choke_group: 1,
            ..Default::default()
        },
    );
    let short = LoadedZone {
        mapping: SampleZone {
            key_low: 61,
            key_high: 61,
            choke_group: 1,
            ..Default::default()
        },
        audio: AudioBuffer::from_interleaved(48000, 1, vec![0.4]),
    };
    let mut unit = sampler(vec![long, short]);
    unit.note_on(60, 1.0);
    render(&mut unit, 256, 64);
    unit.note_on(61, 1.0);
    render(&mut unit, 2, 1);
    assert_eq!(unit.active_voices(), 0);
    let remaining = unit.tail_samples();
    assert!(remaining > 0 && remaining <= 48);
    let pcm = render(&mut unit, remaining + 2, 7);
    assert!(pcm.iter().any(|x| x.abs() > 0.01));
    assert_eq!(unit.tail_samples(), 0);
    assert!(pcm[pcm.len() - 2..].iter().all(|x| *x == 0.0));
}

mod sf2;
