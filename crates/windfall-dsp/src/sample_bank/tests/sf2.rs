use super::*;
use crate::sample_bank::sf2_voice::{self, Controls, Envelope, Modulator, State};

fn fixture(entries: &[(u16, i16)]) -> Bank {
    let mut bank = Bank::new(8192);
    bank.shdr[41] = 0;
    bank.igen = gens(
        &entries
            .iter()
            .map(|&(op, value)| (op, value as u16))
            .chain([(53, 0), (0, 0)])
            .collect::<Vec<_>>(),
    );
    bank.ibag = gens(&[(0, 0), (entries.len() as u16 + 1, 0)]);
    bank
}
fn mods(entries: &[(u16, u16, i16, u16, u16)]) -> Vec<u8> {
    entries
        .iter()
        .flat_map(|&(src, dst, amount, secondary, transform)| {
            [src, dst, amount as u16, secondary, transform]
                .into_iter()
                .flat_map(u16::to_le_bytes)
        })
        .collect()
}
fn disable_defaults(bank: &mut Bank) {
    let entries = sf2_voice::defaults()
        .into_iter()
        .map(|m| (m.source, m.destination, 0, m.amount_source, m.transform))
        .chain([(0, 0, 0, 0, 0)])
        .collect::<Vec<_>>();
    bank.imod = mods(&entries);
    put16(&mut bank.ibag, 6, (entries.len() - 1) as u16);
}
fn parsed(bank: &Bank) -> SampleBank {
    soundfont::parse_soundfont(&bank.bytes()).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.0001, "{a} != {b}");
}

#[test]
fn sf2_every_value_generator_defaults_units_ranges_and_preset_addition() {
    // Enumerate every defined value generator, independently of the production
    // predicate. Preset values offset absolute instrument values in native units.
    let cases = [
        (5, 1200),
        (6, -1200),
        (7, 600),
        (8, 6900),
        (9, 120),
        (10, 2400),
        (11, -2400),
        (13, 60),
        (15, 100),
        (16, 200),
        (17, -100),
        (21, -1200),
        (22, -3600),
        (23, -2400),
        (24, -4800),
        (25, -1200),
        (26, -2400),
        (27, -3600),
        (28, -4800),
        (29, 200),
        (30, -6000),
        (31, 10),
        (32, -10),
        (33, -1200),
        (34, -2400),
        (35, -3600),
        (36, -4800),
        (37, 600),
        (38, -6000),
        (39, 20),
        (40, -20),
        (48, 200),
        (51, 1),
        (52, 10),
        (56, 80),
    ];
    for (op, value) in cases {
        let mut bank = fixture(&[(op, value)]);
        disable_defaults(&mut bank);
        bank.pgen = gens(&[(op, 5), (41, 0), (0, 0)]);
        put16(&mut bank.pbag, 4, 2);
        let prepared = parsed(&bank);
        near(prepared.sf2[0].generators[op as usize], value as f32 + 5.0);
        let resolved = prepared.sf2[0].resolve(60, 127, Controls::default());
        near(resolved[op as usize], value as f32 + 5.0);
    }
    let bank = parsed(&fixture(&[]));
    let g = bank.sf2[0].generators;
    near(g[8], 13500.0);
    near(g[56], 100.0);
    for op in [21, 23, 25, 26, 27, 28, 30, 33, 34, 35, 36, 38] {
        near(g[op], -12000.0);
    }
    for (op, lo, hi) in [
        (8, 1500.0, 13500.0),
        (9, 0.0, 960.0),
        (15, 0.0, 1000.0),
        (17, -500.0, 500.0),
        (29, 0.0, 1000.0),
        (37, 0.0, 1440.0),
        (48, 0.0, 1440.0),
        (56, 0.0, 1200.0),
    ] {
        for (input, expected) in [(-30000, lo), (30000, hi)] {
            let mut b = fixture(&[(op, input)]);
            disable_defaults(&mut b);
            near(
                parsed(&b).sf2[0].resolve(60, 127, Controls::default())[op as usize],
                expected,
            );
        }
    }
}

#[test]
fn sf2_envelope_reference_timecents_centibels_and_key_scaled_hold_decay() {
    let mut bank = fixture(&[
        (25, -1200),
        (26, -2400),
        (27, 0),
        (28, 1200),
        (29, 250),
        (30, -3600),
        (31, 100),
        (32, -100),
        (33, -1200),
        (34, -2400),
        (35, 0),
        (36, 1200),
        (37, 600),
        (38, -3600),
        (39, 100),
        (40, -100),
    ]);
    disable_defaults(&mut bank);
    let prepared = parsed(&bank);
    let g = prepared.sf2[0].resolve(72, 127, Controls::default());
    for volume in [false, true] {
        let env = Envelope::from_values(&g, volume, 72.0);
        near(env.delay, 0.5);
        near(env.attack, 0.25);
        near(env.hold, 0.5);
        near(env.decay, 4.0);
        near(env.release, 0.125);
        near(env.held(0.4, volume), 0.0);
        near(env.held(0.8, volume), 1.0);
        near(env.held(5.25, volume), if volume { 0.001 } else { 0.75 });
        near(
            env.held(3.25, volume),
            if volume {
                10.0_f32.powf(-300.0 / 200.0)
            } else {
                0.875
            },
        );
    }
    let mut zero = fixture(&[
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
        (38, -32768),
    ]);
    disable_defaults(&mut zero);
    let prepared = parsed(&zero);
    let envelope = Envelope::from_values(
        &prepared.sf2[0].resolve(60, 127, Controls::default()),
        true,
        60.0,
    );
    near(envelope.delay, 0.0);
    near(envelope.attack, 0.0);
    near(envelope.held(0.0, true), 1.0);
}

#[test]
fn sf2_global_local_override_and_cross_level_range_intersection() {
    let mut b = fixture(&[]);
    b.pbag = gens(&[(0, 0), (2, 0), (5, 0)]);
    put16(&mut b.phdr, 62, 2);
    b.pgen = gens(&[
        (43, 40 | (90 << 8)),
        (48, 100),
        (43, 50 | (80 << 8)),
        (48, 200),
        (41, 0),
        (0, 0),
    ]);
    b.ibag = gens(&[(0, 0), (3, 0), (7, 0)]);
    put16(&mut b.inst, 42, 2);
    b.igen = gens(&[
        (43, 45 | (100 << 8)),
        (44, 10 | (100 << 8)),
        (48, 50),
        (43, 60 | (100 << 8)),
        (44, 30 | (90 << 8)),
        (48, 80),
        (53, 0),
        (0, 0),
    ]);
    let bank = parsed(&b);
    let z = bank.zones()[0].mapping;
    assert_eq!(
        (z.key_low, z.key_high, z.velocity_low, z.velocity_high),
        (60, 80, 30, 90)
    );
    near(bank.sf2[0].generators[48], 280.0);
    // A preset-only sample generator is ignored; scaleTuning is a value
    // generator and therefore is additive at the preset level.
    let mut b = fixture(&[(46, 60), (47, 100), (54, 3), (56, 50), (57, 7), (58, 72)]);
    b.pgen = gens(&[
        (46, 90),
        (47, 70),
        (54, 1),
        (56, 25),
        (57, 9),
        (58, 40),
        (41, 0),
        (0, 0),
    ]);
    put16(&mut b.pbag, 4, 7);
    let bank = parsed(&b);
    let g = bank.sf2[0].generators;
    near(g[46], 60.0);
    near(g[47], 100.0);
    near(g[56], 75.0);
    assert_eq!(bank.sf2[0].root, 72);
    assert_eq!(bank.sf2[0].mode, 3);
    assert_eq!(bank.zones()[0].mapping.choke_group, 7);
}

#[test]
fn sf2_all_eight_sample_address_offsets_are_frame_units() {
    let mut b = Bank::new(65540);
    b.shdr[41] = 0;
    put32(&mut b.shdr, 28, 0);
    put32(&mut b.shdr, 32, 65540);
    b.igen = gens(&[
        (0, 1),
        (1, (-1i16) as u16),
        (2, 1),
        (3, (-1i16) as u16),
        (4, 1),
        (12, (-1i16) as u16),
        (45, 1),
        (50, (-1i16) as u16),
        (54, 1),
        (53, 0),
        (0, 0),
    ]);
    put16(&mut b.ibag, 4, 10);
    let bank = parsed(&b);
    let z = &bank.zones()[0];
    assert_eq!(z.audio.frames(), 2);
    assert_eq!((z.mapping.loop_start, z.mapping.loop_end), (0, 2));
}

#[test]
fn sf2_default_modulators_replace_at_instrument_and_add_at_preset() {
    let mut b = fixture(&[]);
    // Instrument global replaces default 960; local replaces global 400 with
    // 200. Preset global 100 is overridden by preset local 50, then added.
    b.ibag = gens(&[(0, 0), (0, 1), (1, 2)]);
    put16(&mut b.inst, 42, 2);
    b.imod = mods(&[
        (0x0502, 48, 400, 0, 0),
        (0x0502, 48, 200, 0, 0),
        (0, 0, 0, 0, 0),
    ]);
    b.pbag = gens(&[(0, 0), (0, 1), (1, 2)]);
    put16(&mut b.phdr, 62, 2);
    b.pmod = mods(&[
        (0x0502, 48, 100, 0, 0),
        (0x0502, 48, 50, 0, 0),
        (0, 0, 0, 0, 0),
    ]);
    let bank = parsed(&b);
    let region = &bank.sf2[0];
    let m = region
        .modulators
        .iter()
        .find(|m| m.source == 0x0502)
        .unwrap();
    near(m.amount, 250.0);
    let expected = 250.0 * (-(40.0 / 96.0) * (64.0_f32 / 127.0).log10());
    near(region.resolve(60, 64, Controls::default())[48], expected);
    // An explicit zero suppresses the default completely.
    let mut b = fixture(&[]);
    disable_defaults(&mut b);
    near(
        parsed(&b).sf2[0].resolve(60, 1, Controls::default())[48],
        0.0,
    );
}

#[test]
fn sf2_source_curves_polarities_directions_amount_sources_and_transforms() {
    for kind in 0..4 {
        for bipolar in [false, true] {
            for reverse in [false, true] {
                let op =
                    (kind << 10) | if bipolar { 512 } else { 0 } | if reverse { 256 } else { 0 };
                for x in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                    let shape = |v: f32| match kind {
                        0 => v,
                        1 => {
                            if v == 0.0 {
                                0.0
                            } else {
                                (1.0 + 40.0 / 96.0 * v.log10()).clamp(0.0, 1.0)
                            }
                        }
                        2 => {
                            if v == 1.0 {
                                1.0
                            } else {
                                (-40.0 / 96.0 * (1.0 - v).log10()).clamp(0.0, 1.0)
                            }
                        }
                        _ => {
                            if v >= 0.5 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    let input = if bipolar { x * 2.0 - 1.0 } else { x };
                    let expected = if bipolar {
                        let v = if kind == 3 {
                            if input >= 0.0 { 1.0 } else { -1.0 }
                        } else {
                            input.signum() * shape(input.abs())
                        };
                        if reverse { -v } else { v }
                    } else {
                        let v = shape(input);
                        if reverse { 1.0 - v } else { v }
                    };
                    near(sf2_voice::curve(op, input), expected);
                }
            }
        }
    }
    let mut controls = Controls::default();
    controls.cc[20] = 64;
    controls.cc[21] = 32;
    let m = Modulator {
        source: 0x0094,
        destination: 5,
        amount: 1200.0,
        amount_source: 0x0095,
        transform: 0,
        link: None,
    };
    near(
        m.output(60, 127, controls),
        1200.0 * 64.0 / 127.0 * 32.0 / 127.0,
    );
    let m = Modulator {
        amount: -1200.0,
        transform: 2,
        ..m
    };
    near(
        m.output(60, 127, controls),
        1200.0 * 64.0 / 127.0 * 32.0 / 127.0,
    );
    let mut b = fixture(&[]);
    b.imod = mods(&[(0x0094, 5, 1200, 0x0095, 0), (0, 0, 0, 0, 0)]);
    put16(&mut b.ibag, 6, 1);
    let bank = parsed(&b);
    near(
        bank.sf2[0].resolve(60, 127, controls)[5],
        1200.0 * 64.0 / 127.0 * 32.0 / 127.0,
    );
}

#[test]
fn sf2_all_default_controller_destinations_match_reference_values() {
    let b = fixture(&[]);
    let bank = parsed(&b);
    let region = &bank.sf2[0];
    let mut controls = Controls::default();
    controls.cc[1] = 127;
    controls.pressure = 127;
    controls.cc[7] = 64;
    controls.cc[10] = 32;
    controls.cc[11] = 32;
    controls.cc[91] = 64;
    controls.cc[93] = 32;
    controls.wheel = 12288;
    controls.sensitivity = 12.0;
    let g = region.resolve(60, 64, controls);
    near(g[6], 100.0);
    near(g[8], 13500.0 - 2400.0 * (1.0 - 64.0 / 127.0));
    near(g[17], -500.0);
    near(g[16], 200.0 * 64.0 / 127.0);
    near(g[15], 200.0 * 32.0 / 127.0);
    near(g[59], 600.0);
    let attenuation = -400.0 * ((64.0_f32 / 127.0).log10() * 2.0 + (32.0_f32 / 127.0).log10());
    near(g[48], attenuation);
}

#[test]
fn sf2_voice_measures_fixed_key_velocity_scale_root_tune_and_lfo_pitch_depth() {
    let mut b = fixture(&[
        (46, 72),
        (47, 64),
        (51, 1),
        (52, 50),
        (56, 50),
        (58, 60),
        (5, 1200),
        (6, 600),
        (7, 1200),
        (21, -32768),
        (22, -3600),
        (23, -32768),
        (24, -3600),
        (25, -32768),
        (26, -32768),
        (27, -32768),
        (28, -32768),
        (29, 0),
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
    ]);
    disable_defaults(&mut b);
    let bank = parsed(&b);
    let region = &bank.sf2[0];
    let mut state = State::default();
    state.start(region, 48, 127, Controls::default());
    state.tick(
        region,
        &bank.zones()[0].audio,
        RATE,
        Controls::default(),
        0.0,
        1.0,
        1.0,
        0.0,
    );
    // Fixed key 72 at scale 50 =>600c, coarse/fine=>150c, modEnv=>1200c.
    let step = 2.0_f64.powf(1950.0 / 1200.0);
    assert!((state.position - step).abs() < 1e-6);
    near(sf2_voice::triangle(0.0), 0.0);
    near(sf2_voice::triangle(0.25), 1.0);
    near(sf2_voice::triangle(0.75), -1.0);
    near(sf2_voice::hertz(6900.0), 440.0);
    near(
        sf2_voice::hertz(0.0),
        440.0 * 2.0_f32.powf(-6900.0 / 1200.0),
    );
    near(sf2_voice::amplitude(200.0), 0.1);
}

fn stereo_fixture() -> Bank {
    let mut b = fixture(&[(54, 1)]);
    b.smpl.extend(b.smpl.clone());
    let mut right = b.shdr[..46].to_vec();
    put32(&mut right, 20, 8192);
    put32(&mut right, 24, 16384);
    put32(&mut right, 28, 8193);
    put32(&mut right, 32, 16383);
    put16(&mut right, 42, 0);
    put16(&mut right, 44, 2);
    put16(&mut b.shdr, 42, 1);
    put16(&mut b.shdr, 44, 4);
    b.shdr.splice(46..46, right);
    b.igen = gens(&[
        (17, (-500i16) as u16),
        (54, 1),
        (53, 0),
        (17, 500),
        (54, 1),
        (53, 1),
        (0, 0),
    ]);
    b.ibag = gens(&[(0, 0), (3, 0), (6, 0)]);
    put16(&mut b.inst, 42, 2);
    b
}
#[test]
fn sf2_stereo_linked_zones_make_one_phase_locked_voice_with_24_bit_pcm() {
    let mut b = stereo_fixture();
    b.sm24 = Some(vec![128; 16384]);
    let bank = Arc::new(parsed(&b));
    assert_eq!(bank.zones().len(), 1);
    assert_eq!(bank.zones()[0].audio.channels(), 2);
    near(
        bank.zones()[0].audio.samples()[0],
        ((100i32 << 8) | 128) as f32 / 8388608.0,
    );
    let mut unit = SoundFont::default();
    unit.prepare(RATE, 127);
    unit.install_bank(bank);
    unit.note_on(60, 1.0);
    assert_eq!(unit.active_voices(), 1);
    let mut l = [0.0; 127];
    let mut r = [0.0; 127];
    for _ in 0..100 {
        unit.process(&mut l, &mut r);
        assert_eq!(l, r);
    }
}

#[test]
fn sf2_preset_pan_adds_after_stereo_channel_layout_and_keeps_one_voice() {
    let mut b = stereo_fixture();
    b.pgen = gens(&[(17, 100), (41, 0), (0, 0)]);
    put16(&mut b.pbag, 4, 2);
    let bank = Arc::new(parsed(&b));
    assert_eq!(bank.zones().len(), 1);
    near(bank.sf2[0].generators[17], 100.0);
    let mut unit = SoundFont::default();
    unit.prepare(RATE, 127);
    unit.install_bank(bank);
    unit.note_on(60, 1.0);
    let mut l = [0.0; 127];
    let mut r = [0.0; 127];
    unit.process(&mut l, &mut r);
    assert_eq!(unit.active_voices(), 1);
    assert!(r.iter().any(|sample| sample.abs() > 0.001));
    for frame in 0..127 {
        assert!((l[frame] - 0.8 * r[frame]).abs() < 1e-6);
    }
}

#[test]
fn sf2_negative_24_bit_reconstruction_and_hostile_lengths_links_modes() {
    let mut b = fixture(&[]);
    b.sm24 = Some(vec![255; 8192]);
    b.smpl[..2].copy_from_slice(&(-1i16).to_le_bytes());
    assert_eq!(parsed(&b).zones()[0].audio.samples()[0], -1.0 / 8388608.0);
    b.sm24.as_mut().unwrap().pop();
    assert!(soundfont::parse_soundfont(&b.bytes()).is_err());
    let mut b = stereo_fixture();
    put16(&mut b.shdr, 42, 999);
    assert!(soundfont::parse_soundfont(&b.bytes()).is_err());
    let mut b = stereo_fixture();
    put32(&mut b.shdr, 46 + 36, 24000);
    assert!(soundfont::parse_soundfont(&b.bytes()).is_err());
    for mode in 0..4 {
        let b = fixture(&[(54, mode)]);
        let bank = parsed(&b);
        assert_eq!(
            bank.zones()[0].mapping.loop_mode,
            match mode {
                1 => BankLoop::Continuous,
                3 => BankLoop::UntilRelease,
                _ => BankLoop::Off,
            }
        );
    }
}

#[test]
fn sf2_nonlooping_headers_ignore_unused_loop_points_and_support_low_sample_rates() {
    let mut b = fixture(&[(54, 0)]);
    put32(&mut b.shdr, 20, 100);
    put32(&mut b.shdr, 28, 0);
    put32(&mut b.shdr, 32, 0);
    put32(&mut b.shdr, 36, 400);
    let bank = parsed(&b);
    assert_eq!(bank.zones()[0].audio.sample_rate(), 400);
    assert_eq!(bank.zones()[0].audio.frames(), 8092);
    b.igen = gens(&[(54, 1), (53, 0), (0, 0)]);
    put16(&mut b.ibag, 4, 2);
    assert!(soundfont::parse_soundfont(&b.bytes()).is_err());
}

#[test]
fn sf2_delay_volume_envelope_loop_until_release_exclusive_class_and_partitions() {
    let mut b = fixture(&[
        (33, -1200),
        (34, -32768),
        (35, -32768),
        (36, -32768),
        (38, 1200),
        (54, 3),
        (57, 1),
    ]);
    disable_defaults(&mut b);
    // A half-second delay must not be mistaken for a silent completed voice.
    // Supply enough pre-loop frames to keep the oscillator alive throughout it.
    b.smpl = (0..48000).flat_map(|_| 1000_i16.to_le_bytes()).collect();
    put32(&mut b.shdr, 24, 48000);
    put32(&mut b.shdr, 28, 1);
    put32(&mut b.shdr, 32, 47999);
    let bank = Arc::new(parsed(&b));
    let render = |block: usize| {
        let mut unit = SoundFont::default();
        unit.prepare(RATE, block);
        unit.install_bank(bank.clone());
        unit.note_on(60, 1.0);
        let pcm = super::render(&mut unit, 25000, block);
        assert!(pcm[..24000].iter().all(|x| *x == 0.0));
        assert!(pcm[24100..].iter().any(|x| x.abs() > 0.001));
        unit.note_on(60, 1.0);
        assert_eq!(unit.engine.voices.iter().filter(|v| v.active).count(), 1);
        assert_eq!(unit.engine.choked.iter().filter(|v| v.active).count(), 1);
        pcm
    };
    assert_eq!(render(1), render(127));
    let mut state = State::default();
    let region = &bank.sf2[0];
    state.start(region, 60, 127, Controls::default());
    state.position = 47999.5;
    state.tick(
        region,
        &bank.zones()[0].audio,
        RATE,
        Controls::default(),
        0.0,
        1.0,
        1.0,
        0.0,
    );
    assert!(state.position < 10.0);
    // Use a sounding release value, then the same position proceeds into outro.
    for _ in 0..24010 {
        state.tick(
            region,
            &bank.zones()[0].audio,
            RATE,
            Controls::default(),
            0.0,
            1.0,
            1.0,
            0.0,
        );
    }
    state.release();
    state.position = 47999.5;
    state.tick(
        region,
        &bank.zones()[0].audio,
        RATE,
        Controls::default(),
        0.0,
        1.0,
        1.0,
        0.0,
    );
    assert!(state.position >= 48000.0);
}

#[test]
fn sf2_defined_chorus_reverb_returns_and_immutable_metadata_are_budgeted() {
    let mut b = fixture(&[
        (15, 1000),
        (16, 1000),
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
    ]);
    disable_defaults(&mut b);
    let bytes = b.bytes();
    let budget = Arc::new(BankBudget::new(MAX_BANK_BYTES));
    let bank = soundfont::parse_soundfont_preset_budgeted(&bytes, 3, 7, &budget).unwrap();
    assert!(bank.bytes() > bank.zones()[0].audio.retained_pcm_bytes());
    assert_eq!(budget.used(), bank.bytes());
    let mut sends = sf2_voice::Sends::default();
    sends.prepare(RATE);
    let mut energy = 0.0;
    for frame in 0..48000 {
        let sample = if frame == 0 { 1.0 } else { 0.0 };
        let out = sends.tick([[sample; 2]; 2]);
        if frame > 500 {
            energy += out.iter().map(|x| x * x).sum::<f32>();
        }
    }
    assert!(energy > 0.0001);
    drop(bank);
    assert_eq!(budget.used(), 0);
}

#[test]
fn sf2_voice_filter_resonance_attenuation_and_pan_have_measured_reference_gain() {
    for (attenuation, q, pan) in [
        (0, 0, 0),
        (200, 0, 0),
        (0, 120, 0),
        (0, 0, -500),
        (0, 0, 500),
    ] {
        let mut b = fixture(&[
            (8, 6900),
            (9, q),
            (17, pan),
            (48, attenuation),
            (54, 1),
            (33, -32768),
            (34, -32768),
            (35, -32768),
            (36, -32768),
        ]);
        b.smpl = (0..8192).flat_map(|_| 1000_i16.to_le_bytes()).collect();
        disable_defaults(&mut b);
        let bank = parsed(&b);
        let region = &bank.sf2[0];
        let mut state = State::default();
        state.start(region, 60, 127, Controls::default());
        let mut out = [0.0; 2];
        for _ in 0..4000 {
            out = state.tick(
                region,
                &bank.zones()[0].audio,
                RATE,
                Controls::default(),
                0.0,
                1.0,
                1.0,
                0.0,
            );
        }
        let gain = 1000.0 / 32768.0 * 10.0_f32.powf(-(attenuation as f32 + q as f32 * 0.5) / 200.0);
        near(out[0], gain * ((1.0 - pan as f32 / 500.0) * 0.5).sqrt());
        near(out[1], gain * ((1.0 + pan as f32 / 500.0) * 0.5).sqrt());
    }
    // An independently computed sinusoidal frequency probe sees a low cutoff
    // attenuate above-band audio; resonance produces a peak near the cutoff.
    let energy = |fc: i16, q: i16| {
        let mut b = fixture(&[
            (8, fc),
            (9, q),
            (54, 1),
            (33, -32768),
            (34, -32768),
            (35, -32768),
            (36, -32768),
        ]);
        b.smpl = (0..8192)
            .flat_map(|i| {
                ((0.25 * (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin() * 32767.0) as i16)
                    .to_le_bytes()
            })
            .collect();
        disable_defaults(&mut b);
        let bank = parsed(&b);
        let mut state = State::default();
        state.start(&bank.sf2[0], 60, 127, Controls::default());
        (0..8000)
            .map(|i| {
                let out = state.tick(
                    &bank.sf2[0],
                    &bank.zones()[0].audio,
                    RATE,
                    Controls::default(),
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                );
                if i < 4000 { 0.0 } else { out[0] * out[0] }
            })
            .sum::<f32>()
    };
    assert!(energy(4500, 0) < energy(13500, 0) * 0.02);
    assert!(energy(6900, 120) > energy(6900, 0) * 2.0);
}

#[test]
fn sf2_lfo_delay_frequency_and_pitch_filter_volume_routes_match_computed_steps() {
    let mut b = fixture(&[
        (5, 1200),
        (6, 600),
        (10, 1200),
        (13, 200),
        (21, -1200),
        (22, -3600),
        (23, -1200),
        (24, -3600),
        (54, 1),
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
    ]);
    disable_defaults(&mut b);
    let bank = parsed(&b);
    let region = &bank.sf2[0];
    let mut state = State::default();
    state.start(region, 60, 127, Controls::default());
    for frame in 0..30000 {
        let before = state.position;
        state.tick(
            region,
            &bank.zones()[0].audio,
            RATE,
            Controls::default(),
            0.0,
            1.0,
            1.0,
            0.0,
        );
        let phase = if frame < 24000 {
            0.0
        } else {
            (frame - 24000) as f32 * (440.0 * 2.0_f32.powf(-10500.0 / 1200.0)) / RATE
        };
        let p = phase.fract();
        let triangle = if p < 0.25 {
            p * 4.0
        } else if p < 0.75 {
            2.0 - p * 4.0
        } else {
            p * 4.0 - 4.0
        };
        if before < 8190.0 {
            let expected = 2.0_f64.powf((triangle as f64 * 1800.0) / 1200.0);
            assert!(
                (state.position - before - expected).abs() < 0.0001,
                "frame {frame}"
            );
        }
    }
}

#[test]
fn sf2_raw_midi_rpn_pitch_pressure_arbitrary_cc_and_sustain_reach_sounding_voices() {
    let b = fixture(&[(54, 3), (38, 0)]);
    let bank = Arc::new(parsed(&b));
    let mut unit = SoundFont::default();
    unit.prepare(RATE, 64);
    unit.install_bank(bank);
    unit.note_on(60, 1.0);
    unit.midi(SoundFontMidi::Controller {
        number: 101,
        value: 0,
    });
    unit.midi(SoundFontMidi::Controller {
        number: 100,
        value: 0,
    });
    unit.midi(SoundFontMidi::Controller {
        number: 6,
        value: 12,
    });
    unit.midi(SoundFontMidi::Controller {
        number: 38,
        value: 50,
    });
    unit.midi(SoundFontMidi::PitchWheel(12288));
    unit.midi(SoundFontMidi::Pressure(127));
    unit.midi(SoundFontMidi::Controller {
        number: 1,
        value: 127,
    });
    unit.midi(SoundFontMidi::Controller {
        number: 20,
        value: 99,
    });
    unit.midi(SoundFontMidi::PolyPressure {
        key: 60,
        value: 100,
    });
    let mut l = [0.0; 64];
    let mut r = [0.0; 64];
    unit.process(&mut l, &mut r);
    near(unit.engine.voices[0].sf2.values_for_test()[59], 625.0);
    near(unit.engine.voices[0].sf2.values_for_test()[6], 100.0);
    assert_eq!(unit.engine.sf2_controls.cc[20], 99);
    assert_eq!(unit.engine.sf2_poly[60], 100);
    let mut p = SoundFontParams::default();
    p.level = 0.75;
    unit.set_params(&p);
    assert_eq!(unit.engine.sf2_controls.cc[20], 99);
    near(unit.engine.sf2_controls.sensitivity, 12.5);
    unit.midi(SoundFontMidi::Controller {
        number: 64,
        value: 127,
    });
    unit.note_off(60);
    assert!(!unit.engine.voices[0].releasing);
    unit.midi(SoundFontMidi::Controller {
        number: 64,
        value: 0,
    });
    assert!(unit.engine.voices[0].releasing);
}

#[test]
fn sf2_linked_modulator_graph_sums_inputs_ignores_cycles_and_keeps_composition() {
    let mut b = fixture(&[]);
    b.imod = mods(&[
        (0x0094, 0x8002, 1, 0, 0),
        (0x0095, 0x8002, 1, 0, 0),
        (127, 5, 1200, 0x0096, 0),
        (0, 0, 0, 0, 0),
    ]);
    put16(&mut b.ibag, 6, 3);
    let mut controls = Controls::default();
    controls.cc[20] = 32;
    controls.cc[21] = 16;
    controls.cc[22] = 64;
    let bank = parsed(&b);
    let g = bank.sf2[0].resolve(60, 127, controls);
    near(g[5], 1200.0 * ((32.0 + 16.0) / 127.0) * (64.0 / 127.0));
    let budget = Arc::new(BankBudget::new(MAX_BANK_BYTES));
    let bank = soundfont::parse_soundfont_preset_budgeted(&b.bytes(), 3, 7, &budget).unwrap();
    assert_eq!(
        bank.bytes() + bank.sf2[0].programs[0].bytes(),
        budget.used()
    );
    // A cycle feeds a destination, while an independent route remains usable.
    b.imod = mods(&[
        (127, 0x8001, 1, 0, 0),
        (127, 0x8000, 1, 0, 0),
        (0x0094, 8, -1200, 0, 0),
        (0, 0, 0, 0, 0),
    ]);
    let bank = parsed(&b);
    near(
        bank.sf2[0].resolve(60, 127, controls)[8],
        13500.0 - 1200.0 * 32.0 / 127.0,
    );
    assert_eq!(bank.sf2[0].programs.len(), 0);
}

#[test]
fn sf2_modulation_envelope_filter_and_lfo_volume_routes_match_pcm_reference() {
    let mut b = fixture(&[
        (8, 6900),
        (11, 1200),
        (48, 200),
        (25, -32768),
        (26, -32768),
        (27, -32768),
        (28, -32768),
        (29, 0),
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
    ]);
    b.smpl = (0..8192).flat_map(|_| 1000_i16.to_le_bytes()).collect();
    disable_defaults(&mut b);
    let bank = parsed(&b);
    let mut state = State::default();
    state.start(&bank.sf2[0], 60, 127, Controls::default());
    let out = state.tick(
        &bank.sf2[0],
        &bank.zones()[0].audio,
        RATE,
        Controls::default(),
        0.0,
        1.0,
        1.0,
        0.0,
    );
    // A full-scale +1200c modulation envelope doubles 440Hz. Independently
    // compute the Butterworth first impulse coefficient using tan/prewarping,
    // then the 200cB amplifier and center pan. No renderer coefficient is read.
    let k = (std::f64::consts::PI * 880.0 / 48000.0).tan();
    let b0 = k * k / (1.0 + std::f64::consts::SQRT_2 * k + k * k);
    let reference = (1000.0 / 32768.0 * b0 * 0.1 * std::f64::consts::FRAC_1_SQRT_2) as f32;
    assert!((out[0] - reference).abs() < 1e-8);
    let mut b = fixture(&[
        (13, 200),
        (21, -32768),
        (22, 0),
        (54, 1),
        (33, -32768),
        (34, -32768),
        (35, -32768),
        (36, -32768),
    ]);
    b.smpl = (0..8192).flat_map(|_| 1000_i16.to_le_bytes()).collect();
    disable_defaults(&mut b);
    let bank = parsed(&b);
    let mut state = State::default();
    state.start(&bank.sf2[0], 60, 127, Controls::default());
    let frame = 1470;
    let mut out = [0.0; 2];
    for _ in 0..=frame {
        out = state.tick(
            &bank.sf2[0],
            &bank.zones()[0].audio,
            RATE,
            Controls::default(),
            0.0,
            1.0,
            1.0,
            0.0,
        );
    }
    let phase = frame as f64 * (440.0 * 2.0_f64.powf(-6900.0 / 1200.0)) / 48000.0;
    let lfo = if phase < 0.25 {
        phase * 4.0
    } else {
        2.0 - phase * 4.0
    };
    let reference =
        (1000.0 / 32768.0 * std::f64::consts::FRAC_1_SQRT_2 * 10.0_f64.powf(-lfo)) as f32;
    assert!((out[0] - reference).abs() < 1e-6);
}
