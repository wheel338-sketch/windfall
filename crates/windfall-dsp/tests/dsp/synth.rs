//! The subtractive synth: pitch, envelopes, voices, filter and aliasing.

use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::{
    EnvelopeParams, FilterMode, FilterSlope, Instrument, LfoShape, MAX_POLYPHONY, SubtractiveSynth,
    SynthParams, VoiceMode, Waveform,
};

use crate::support::{
    RATE, RATES, assert_finite, cents, db, frequency_of, peak, prepared_instrument, random_params,
    render, rms, sharp_magnitudes, steepest, tone_level, windowed_magnitudes,
};

/// A patch with nothing in the way of measuring: one oscillator at full
/// level, an envelope that is simply on while the key is down, the filter
/// wide open with no resonance, and no velocity response.
fn plain(waveform: Waveform) -> SynthParams {
    let mut params = SynthParams::default();
    params.oscillators[0].waveform = waveform;
    params.amp_envelope = EnvelopeParams {
        attack_ms: 0.0,
        decay_ms: 1.0,
        sustain: 1.0,
        release_ms: 5.0,
    };
    params.filter.resonance = 0.0;
    params.amp_velocity = 0.0;
    params.gain = 1.0;
    params
}

fn synth(params: &SynthParams, rate: f32) -> SubtractiveSynth {
    prepared_instrument(params, rate)
}

fn key_hz(key: f64) -> f64 {
    440.0 * 2.0_f64.powf((key - 69.0) / 12.0)
}

/// Plays one key and returns `seconds` of the left output, starting after
/// the note has settled.
fn held_note(params: &SynthParams, key: u8, rate: f32, seconds: f32) -> Vec<f32> {
    let mut synth = synth(params, rate);
    synth.note_on(key, 1.0);
    render(&mut synth, (rate * 0.1) as usize);
    render(&mut synth, (rate * seconds) as usize).0
}

#[test]
fn concert_a_is_440_hz_and_every_key_is_within_a_cent() {
    for rate in RATES {
        for waveform in [Waveform::Sine, Waveform::Saw, Waveform::Triangle] {
            for key in [21_u8, 45, 69, 93, 108] {
                let note = held_note(&plain(waveform), key, rate, 1.0);
                let error = cents(frequency_of(&note, rate), key_hz(f64::from(key)));
                assert!(
                    error.abs() < 1.0,
                    "{waveform:?} key {key} at {rate}: {error} cents off"
                );
            }
        }
    }
    let note = held_note(&plain(Waveform::Sine), 69, RATE, 2.0);
    let error = cents(frequency_of(&note, RATE), 440.0);
    assert!(error.abs() < 0.05, "A4 is {error} cents off");
}

#[test]
fn coarse_and_fine_tune_the_oscillator() {
    let mut params = plain(Waveform::Sine);
    params.oscillators[0].coarse = 12;
    let note = held_note(&params, 69, RATE, 1.0);
    assert!(cents(frequency_of(&note, RATE), 880.0).abs() < 0.2);

    params.oscillators[0].coarse = -7;
    params.oscillators[0].fine_cents = 35.0;
    let note = held_note(&params, 69, RATE, 1.0);
    assert!(cents(frequency_of(&note, RATE), key_hz(62.35)).abs() < 0.2);
}

/// The level of `signal` over each window of `window` samples.
fn envelope(signal: &[f32], window: usize) -> Vec<f32> {
    signal.chunks(window).map(peak).collect()
}

#[test]
fn the_amp_envelope_keeps_its_times() {
    for rate in RATES {
        let mut params = plain(Waveform::Sine);
        params.amp_envelope = EnvelopeParams {
            attack_ms: 50.0,
            decay_ms: 100.0,
            sustain: 0.4,
            release_ms: 200.0,
        };
        let mut synth = synth(&params, rate);
        let latency = synth.latency_samples();
        assert_eq!(latency, 12);
        // 2 kHz, so that a window of one millisecond holds two full cycles.
        synth.note_on(95, 1.0);
        let ms = (rate / 1_000.0) as usize;
        let (held, _) = render(&mut synth, latency + 400 * ms);
        let level = envelope(&held[latency..], ms);
        // Rising for 50 ms: about two thirds of the way at the halfway
        // point, full at the end.
        assert!(level[0] < 0.1);
        assert!((0.55..=0.8).contains(&level[25]), "{}", level[25]);
        assert!(
            level[48] > 0.95 && level[49] > 0.97,
            "{} {}",
            level[48],
            level[49]
        );
        assert!(level.iter().all(|level| *level < 1.02));
        // Falling to the sustain level over the next 100 ms, steeply at
        // first: halfway through, nearly all of the drop is done.
        assert!(level[60] < 0.9 && level[60] > 0.5, "{}", level[60]);
        assert!(level[100] < 0.45 && level[100] > 0.4, "{}", level[100]);
        assert!((level[152] - 0.4).abs() < 0.01, "{}", level[152]);
        assert!((level[399] - 0.4).abs() < 0.005);

        synth.note_off(95);
        let (released, _) = render(&mut synth, 300 * ms);
        let level = envelope(&released[latency..], ms);
        assert!(level[100] > 0.001 && level[100] < 0.1, "{}", level[100]);
        assert!(level[195] > 1.0e-5, "{}", level[195]);
        assert!(level[203] < 1.0e-4, "{}", level[203]);
        assert_eq!(synth.active_voices(), 0);
    }
}

#[test]
fn silence_is_exact_before_any_note_and_after_the_last() {
    let mut synth = synth(&SynthParams::default(), RATE);
    let (left, right) = render(&mut synth, 2_000);
    assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
    assert_eq!(synth.active_voices(), 0);

    synth.note_on(60, 0.8);
    assert_eq!(synth.active_voices(), 1);
    let (left, _) = render(&mut synth, 4_800);
    assert!(peak(&left) > 0.05);
    synth.note_off(60);
    // The default release is 150 ms.
    render(&mut synth, 7_300);
    assert_eq!(synth.active_voices(), 0);
    let tail = synth.tail_samples();
    render(&mut synth, tail);
    let (left, right) = render(&mut synth, 2_000);
    assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
}

#[test]
fn the_default_patch_is_audible_and_leaves_headroom() {
    let mut synth = synth(&SynthParams::default(), RATE);
    synth.note_on(60, 1.0);
    let (left, right) = render(&mut synth, 24_000);
    assert!(left == right);
    assert!((0.15..=0.4).contains(&peak(&left)), "{}", peak(&left));
    // Six notes struck together at full velocity, all starting in phase,
    // which is the worst case, peak at about full scale.
    for key in [48, 55, 64, 67, 72] {
        synth.note_on(key, 1.0);
    }
    let (left, _) = render(&mut synth, 24_000);
    assert!(peak(&left) < 1.2, "{}", peak(&left));
}

/// The loudest component of a note that is not one of its overtones, in dB
/// relative to its fundamental, looking below `below_hz`.
fn worst_alias_db(params: &SynthParams, key: u8, rate: f32, below_hz: f64) -> f64 {
    let size = 1 << 15;
    let mut synth = synth(params, rate);
    synth.note_on(key, 1.0);
    render(&mut synth, 4_800);
    let (note, _) = render(&mut synth, size);
    let spectrum = sharp_magnitudes(&note);
    let bin_hz = f64::from(rate) / size as f64;
    let fundamental = key_hz(f64::from(key));
    let reference = tone_level(&note, fundamental, rate);
    let mut worst = 0.0_f64;
    for (bin, level) in spectrum.iter().enumerate() {
        let frequency = bin as f64 * bin_hz;
        if frequency < 30.0 || frequency > below_hz {
            continue;
        }
        // Skip the overtones themselves and the few bins the window
        // spreads each of them over.
        let nearest = (frequency / fundamental).round() * fundamental;
        if (frequency - nearest).abs() < 7.0 * bin_hz {
            continue;
        }
        worst = worst.max(*level);
    }
    db(worst / reference)
}

#[test]
fn aliasing_stays_inaudible_even_for_the_highest_notes() {
    // Stated bound: with a pitched waveform at any key, nothing that is
    // not an overtone rises above -80 dB relative to the fundamental
    // anywhere below 16 kHz.
    for rate in [44_100.0, 48_000.0] {
        for waveform in [
            Waveform::Saw,
            Waveform::Square,
            Waveform::Pulse,
            Waveform::Triangle,
        ] {
            let mut params = plain(waveform);
            params.oscillators[0].pulse_width = 0.27;
            for key in [84_u8, 96, 101, 108, 115] {
                let worst = worst_alias_db(&params, key, rate, 16_000.0);
                assert!(
                    worst < -80.0,
                    "{waveform:?} key {key} at {rate}: aliasing at {worst} dB"
                );
            }
        }
    }
    // The bound holds over the whole audible band for all but the top
    // octave of the keyboard.
    let worst = worst_alias_db(&plain(Waveform::Saw), 84, 48_000.0, 20_000.0);
    assert!(worst < -80.0, "{worst}");
}

/// Level of each of the first overtones of a low saw through the filter,
/// relative to the same note with the filter wide open.
fn filter_response(params: &SynthParams, overtones: &[u32]) -> Vec<f64> {
    let key = 33;
    let fundamental = key_hz(f64::from(key));
    let mut open = *params;
    open.filter.cutoff_hz = 20_000.0;
    open.filter.resonance = 0.0;
    open.filter.mode = FilterMode::LowPass;
    open.filter.slope = FilterSlope::Db12;
    let filtered = held_note(params, key, RATE, 1.0);
    let reference = held_note(&open, key, RATE, 1.0);
    overtones
        .iter()
        .map(|overtone| {
            let frequency = fundamental * f64::from(*overtone);
            let size = 32_768;
            db(tone_level(&filtered[..size], frequency, RATE)
                / tone_level(&reference[..size], frequency, RATE))
        })
        .collect()
}

/// The response of an ideal second-order low-pass at `ratio` times its
/// cutoff.
fn ideal_low_pass_db(ratio: f64, q: f64) -> f64 {
    -10.0 * ((1.0 - ratio * ratio).powi(2) + (ratio / q).powi(2)).log10()
}

#[test]
fn the_filter_cuts_at_its_cutoff_with_the_right_slope() {
    // A 55 Hz saw has overtones every 55 Hz. With the cutoff on the 20th
    // overtone, the 10th, 20th, 40th and 80th are half, one, two and four
    // times the cutoff.
    let overtones = [10, 20, 40, 80];
    let mut params = plain(Waveform::Saw);
    params.filter.cutoff_hz = 1_100.0;

    let response = filter_response(&params, &overtones);
    for (overtone, measured) in overtones.iter().zip(&response) {
        let expected =
            ideal_low_pass_db(f64::from(*overtone) / 20.0, std::f64::consts::FRAC_1_SQRT_2);
        assert!(
            (measured - expected).abs() < 0.4,
            "12 dB low-pass, overtone {overtone}: {measured} against {expected}"
        );
    }
    assert!((response[1] + 3.01).abs() < 0.3);
    assert!((response[3] - response[2] + 12.0).abs() < 0.6);

    params.filter.slope = FilterSlope::Db24;
    let response = filter_response(&params, &overtones);
    assert!(
        (response[1] + 3.01).abs() < 0.3,
        "24 dB corner: {}",
        response[1]
    );
    assert!(
        (response[3] - response[2] + 24.0).abs() < 1.2,
        "{response:?}"
    );

    params.filter.slope = FilterSlope::Db12;
    params.filter.mode = FilterMode::HighPass;
    let response = filter_response(&params, &[5, 10, 20, 80]);
    assert!(
        (response[1] - response[0] - 12.0).abs() < 0.6,
        "{response:?}"
    );
    assert!((response[2] + 3.01).abs() < 0.3);
    assert!(response[3].abs() < 0.5);

    params.filter.mode = FilterMode::BandPass;
    let response = filter_response(&params, &[5, 20, 80]);
    // Unity at the cutoff, falling 6 dB per octave either side.
    assert!(response[1].abs() < 0.3, "{response:?}");
    assert!(
        (response[0] - response[2]).abs() < 1.0 && response[0] < -8.0,
        "{response:?}"
    );
}

#[test]
fn resonance_peaks_at_the_cutoff() {
    let mut params = plain(Waveform::Saw);
    params.filter.cutoff_hz = 1_100.0;
    for (resonance, q) in [(0.5_f32, 3.76_f64), (1.0, 20.0)] {
        params.filter.resonance = resonance;
        let response = filter_response(&params, &[2, 20]);
        // Relative to the passband, the peak at the cutoff is the Q. The
        // filter's input is turned down as resonance rises, which moves
        // both by the same amount.
        let peak = response[1] - response[0];
        assert!(
            (peak - 20.0 * q.log10()).abs() < 1.0,
            "resonance {resonance}: peak of {peak} dB"
        );
        let trim = -20.0 * (1.0 + f64::from(resonance)).log10();
        assert!((response[0] - trim).abs() < 0.3, "{}", response[0]);
    }
    // In 24 dB mode the two stages share the peak instead of doubling it.
    params.filter.slope = FilterSlope::Db24;
    params.filter.resonance = 1.0;
    let response = filter_response(&params, &[2, 20]);
    assert!((response[1] - response[0] - 26.0).abs() < 1.0);
}

#[test]
fn key_tracking_envelope_and_velocity_move_the_cutoff() {
    // Each of these should move the cutoff by a known number of octaves.
    // Sixty overtones of a low note show where the -3 dB point lies.
    let corner_hz = |params: &SynthParams, key: u8, velocity: f32| {
        let mut synth = synth(params, RATE);
        synth.note_on(key, velocity);
        render(&mut synth, 9_600);
        let (note, _) = render(&mut synth, 32_768);
        let mut open = *params;
        open.filter = plain(Waveform::Saw).filter;
        let mut reference = self::synth(&open, RATE);
        reference.note_on(key, velocity);
        render(&mut reference, 9_600);
        let (reference, _) = render(&mut reference, 32_768);
        let fundamental = key_hz(f64::from(key));
        let response = |overtone: u32| {
            let frequency = fundamental * f64::from(overtone);
            db(tone_level(&note, frequency, RATE) / tone_level(&reference, frequency, RATE))
        };
        // The first overtone more than 3 dB down, and the crossing
        // between it and the one before, on a logarithmic scale.
        let past = (2..300)
            .find(|overtone| response(*overtone) < -3.0)
            .expect("the filter never cuts");
        let (before, after) = (response(past - 1), response(past));
        let share = (before + 3.0) / (before - after);
        fundamental * f64::from(past - 1) * (f64::from(past) / f64::from(past - 1)).powf(share)
    };
    let mut params = plain(Waveform::Saw);
    params.filter.cutoff_hz = 800.0;
    params.filter_envelope.sustain = 1.0;
    let octaves = |hz: f64, base: f64| (hz / base).log2();
    let base = corner_hz(&params, 36, 1.0);
    assert!(octaves(base, 800.0).abs() < 0.15, "{base}");

    // Full key tracking: a key two octaves below middle C quarters the
    // cutoff.
    params.filter.key_tracking = 1.0;
    let tracked = corner_hz(&params, 36, 1.0);
    assert!((octaves(tracked, base) + 2.0).abs() < 0.2, "{tracked}");
    params.filter.key_tracking = 0.0;

    // The envelope at its sustain of 1 opens the filter by its amount.
    params.filter.envelope_octaves = 2.0;
    let opened = corner_hz(&params, 36, 1.0);
    assert!((octaves(opened, base) - 2.0).abs() < 0.2, "{opened}");
    params.filter.envelope_octaves = 0.0;

    // Full filter velocity: half velocity is two octaves darker.
    params.filter.velocity = 1.0;
    params.filter.cutoff_hz = 3_200.0;
    let soft = corner_hz(&params, 36, 0.5);
    assert!((octaves(soft, 800.0)).abs() < 0.2, "{soft}");
}

#[test]
fn the_filter_survives_any_cutoff_resonance_and_drive() {
    let mut rng = Rng::new(5);
    for rate in [22_050.0, 44_100.0, 96_000.0, 192_000.0] {
        let mut params = plain(Waveform::Saw);
        params.oscillators[1].waveform = Waveform::WhiteNoise;
        params.oscillators[1].level = 1.0;
        params.lfos[0].cutoff_octaves = 5.0;
        params.lfos[0].rate_hz = 30.0;
        params.lfos[0].shape = LfoShape::Random;
        params.filter.envelope_octaves = 8.0;
        let mut synth = synth(&params, rate);
        for key in [0, 40, 80, 127] {
            synth.note_on(key, 1.0);
        }
        let mut loudest = 0.0_f32;
        for step in 0..600 {
            params.filter.cutoff_hz = if step % 2 == 0 { 20.0 } else { 20_000.0 };
            params.filter.resonance = rng.unipolar().round();
            params.filter.drive = rng.unipolar();
            params.filter.slope = if step % 3 == 0 {
                FilterSlope::Db24
            } else {
                FilterSlope::Db12
            };
            params.filter.mode = [
                FilterMode::LowPass,
                FilterMode::BandPass,
                FilterMode::HighPass,
            ][step % 3];
            synth.set_params(&params);
            let (left, right) = render(&mut synth, 37);
            assert_finite(&left, "filter stress");
            loudest = loudest.max(peak(&left)).max(peak(&right));
        }
        // Four notes of two oscillators each, through a filter that can
        // ring 26 dB up.
        assert!(loudest < 400.0, "{loudest} at {rate}");
    }
}

#[test]
fn drive_adds_overtones_and_is_clean_at_zero() {
    let third = |drive: f32| {
        let mut params = plain(Waveform::Sine);
        params.filter.drive = drive;
        let note = held_note(&params, 57, RATE, 1.0);
        (
            tone_level(&note[..32_768], 220.0, RATE),
            tone_level(&note[..32_768], 660.0, RATE),
        )
    };
    let (clean, clean_third) = third(0.0);
    assert!((clean - 1.0).abs() < 0.01);
    assert!(db(clean_third / clean) < -90.0);
    let (barely, barely_third) = third(0.01);
    assert!((barely - 1.0).abs() < 0.02, "{barely}");
    assert!(db(barely_third / barely) < -60.0);
    let (hot, hot_third) = third(1.0);
    assert!(db(hot_third / hot) > -14.0);
    assert!(hot > 0.8 && hot < 2.0, "{hot}");
}

#[test]
fn velocity_sets_the_level_as_far_as_the_patch_asks() {
    let level = |amp_velocity: f32, velocity: f32| {
        let mut params = plain(Waveform::Sine);
        params.amp_velocity = amp_velocity;
        let mut synth = synth(&params, RATE);
        synth.note_on(69, velocity);
        render(&mut synth, 4_800);
        f64::from(peak(&render(&mut synth, 4_800).0))
    };
    assert!((level(0.0, 0.25) - 1.0).abs() < 0.01);
    assert!((level(1.0, 0.25) - 0.25).abs() < 0.01);
    assert!((level(1.0, 1.0) - 1.0).abs() < 0.01);
    assert!((level(0.5, 0.5) - 0.75).abs() < 0.01);
}

#[test]
fn gain_and_pan_place_the_output() {
    let mut params = plain(Waveform::Sine);
    params.gain = 0.5;
    params.pan = -1.0;
    let mut hard_left = synth(&params, RATE);
    hard_left.note_on(69, 1.0);
    let (left, right) = render(&mut hard_left, 9_600);
    assert!((peak(&left[4_800..]) - 0.5).abs() < 0.01);
    assert_eq!(peak(&right), 0.0);

    params.pan = 0.5;
    let mut right_of_centre = synth(&params, RATE);
    right_of_centre.note_on(69, 1.0);
    let (left, right) = render(&mut right_of_centre, 9_600);
    assert!((peak(&left[4_800..]) - 0.25).abs() < 0.01);
    assert!((peak(&right[4_800..]) - 0.5).abs() < 0.01);

    // An oscillator's own pan works the same way.
    let mut params = plain(Waveform::Sine);
    params.oscillators[0].pan = 1.0;
    let mut panned = synth(&params, RATE);
    panned.note_on(69, 1.0);
    let (left, right) = render(&mut panned, 9_600);
    assert_eq!(peak(&left), 0.0);
    assert!((peak(&right[4_800..]) - 1.0).abs() < 0.01);
}

#[test]
fn oscillators_mix_and_noise_has_no_pitch() {
    let mut params = plain(Waveform::Sine);
    params.oscillators[1].waveform = Waveform::Sine;
    params.oscillators[1].level = 0.5;
    params.oscillators[1].coarse = 7;
    params.oscillators[2].waveform = Waveform::Sine;
    params.oscillators[2].level = 0.25;
    params.oscillators[2].coarse = 12;
    let note = held_note(&params, 69, RATE, 1.0);
    let window = &note[..32_768];
    assert!((tone_level(window, 440.0, RATE) - 1.0).abs() < 0.01);
    assert!((tone_level(window, key_hz(76.0), RATE) - 0.5).abs() < 0.01);
    assert!((tone_level(window, 880.0, RATE) - 0.25).abs() < 0.01);

    for (waveform, tilt) in [(Waveform::WhiteNoise, 0.0), (Waveform::PinkNoise, -3.0)] {
        let low = held_note(&plain(waveform), 36, RATE, 2.0);
        let high = held_note(&plain(waveform), 96, RATE, 2.0);
        // The key makes no difference to the level, and the spectrum is
        // flat for white and falls 3 dB per octave for pink.
        assert!(db(rms(&low) / rms(&high)).abs() < 0.5, "{waveform:?}");
        let spectrum = windowed_magnitudes(&low[..65_536]);
        let band = |from_hz: f64, to_hz: f64| {
            let bin = |hz: f64| (hz * 65_536.0 / 48_000.0) as usize;
            let slice = &spectrum[bin(from_hz)..bin(to_hz)];
            10.0 * (slice.iter().map(|m| m * m).sum::<f64>() / slice.len() as f64).log10()
        };
        let per_octave = (band(4_000.0, 8_000.0) - band(250.0, 500.0)) / 4.0;
        assert!(
            (per_octave - tilt).abs() < 0.8,
            "{waveform:?}: {per_octave} dB per octave"
        );
    }
}

#[test]
fn unison_stacks_detuned_copies_across_the_stereo_field() {
    let mut params = plain(Waveform::Sine);
    params.unison_voices = 3;
    params.unison_detune_cents = 50.0;
    params.unison_spread = 1.0;
    let mut stacked = synth(&params, RATE);
    stacked.note_on(69, 1.0);
    render(&mut stacked, 4_800);
    let (left, right) = render(&mut stacked, 131_072);
    // Three copies: 50 cents down on the left, in tune in the middle, 50
    // cents up on the right, each at 1 / sqrt(3) of the level.
    let share = 1.0 / 3.0_f64.sqrt();
    let (down, up) = (key_hz(68.5), key_hz(69.5));
    assert!((tone_level(&left, down, RATE) - share).abs() < 0.02);
    assert!(tone_level(&right, down, RATE) < 0.01);
    assert!((tone_level(&right, up, RATE) - share).abs() < 0.02);
    assert!(tone_level(&left, up, RATE) < 0.01);
    assert!((tone_level(&left, 440.0, RATE) - share).abs() < 0.02);
    assert!((tone_level(&right, 440.0, RATE) - share).abs() < 0.02);

    // With no spread everything is in the middle.
    params.unison_spread = 0.0;
    let mut centred = synth(&params, RATE);
    centred.note_on(69, 1.0);
    let (left, right) = render(&mut centred, 20_000);
    assert!(left == right);

    // A full stack is about as loud as a single oscillator.
    params.unison_voices = 7;
    params.unison_spread = 0.5;
    let mut full = synth(&params, RATE);
    full.note_on(69, 1.0);
    let (left, _) = render(&mut full, 96_000);
    let single = held_note(&plain(Waveform::Sine), 69, RATE, 1.0);
    assert!(db(rms(&left[9_600..]) / rms(&single)).abs() < 3.0);
}

#[test]
fn a_note_past_the_polyphony_limit_takes_the_oldest_voice_without_a_click() {
    let mut params = plain(Waveform::Sine);
    params.polyphony = 4;
    params.gain = 0.2;
    let mut synth = synth(&params, RATE);
    let keys = [57_u8, 60, 64, 67, 71, 74];
    let mut left = Vec::new();
    for key in keys {
        synth.note_on(key, 1.0);
        left.extend(render(&mut synth, 2_400).0);
    }
    // Four sound; the two that were stolen have finished fading.
    assert_eq!(synth.active_voices(), 4);
    left.extend(render(&mut synth, 32_768).0);
    let settled = &left[left.len() - 32_768..];
    for key in &keys[..2] {
        assert!(
            tone_level(settled, key_hz(f64::from(*key)), RATE) < 1.0e-4,
            "key {key}"
        );
    }
    for key in &keys[2..] {
        let level = tone_level(settled, key_hz(f64::from(*key)), RATE);
        assert!((level - 0.2).abs() < 0.01, "key {key}: {level}");
    }
    // Nothing in the whole performance moves faster than the notes
    // themselves do. A voice cut off abruptly would.
    let natural: f32 = keys
        .iter()
        .map(|key| 0.2 * std::f32::consts::TAU * key_hz(f64::from(*key)) as f32 / RATE)
        .sum();
    assert!(
        steepest(&left) < 1.2 * natural,
        "{} against {natural}",
        steepest(&left)
    );

    // Right after a steal the stolen voice is still fading out.
    synth.note_on(77, 1.0);
    assert_eq!(synth.active_voices(), 5);
    render(&mut synth, 480);
    assert_eq!(synth.active_voices(), 4);
}

#[test]
fn a_released_voice_is_stolen_before_a_held_one() {
    let mut params = plain(Waveform::Sine);
    params.polyphony = 2;
    params.amp_envelope.release_ms = 2_000.0;
    let mut synth = synth(&params, RATE);
    synth.note_on(60, 1.0);
    synth.note_on(64, 1.0);
    render(&mut synth, 2_400);
    synth.note_off(64);
    render(&mut synth, 2_400);
    synth.note_on(67, 1.0);
    render(&mut synth, 4_800);
    let (note, _) = render(&mut synth, 16_384);
    // The older note is still held, so it stays. The newer one had been
    // let go, so it is the one that makes room.
    assert!(tone_level(&note, key_hz(60.0), RATE) > 0.9);
    assert!(tone_level(&note, key_hz(64.0), RATE) < 1.0e-3);
    assert!(tone_level(&note, key_hz(67.0), RATE) > 0.9);
}

#[test]
fn a_burst_of_more_notes_than_there_are_voices_is_survived() {
    let mut params = plain(Waveform::Saw);
    params.polyphony = MAX_POLYPHONY as u8;
    params.unison_voices = 2;
    let mut synth = synth(&params, RATE);
    let mut left = Vec::new();
    for round in 0..40_u8 {
        for key in 0..30_u8 {
            synth.note_on(20 + (key * 3 + round) % 90, 0.7);
        }
        assert!(synth.active_voices() <= MAX_POLYPHONY + 8);
        left.extend(render(&mut synth, 16).0);
    }
    assert_finite(&left, "burst");
    render(&mut synth, 2_000);
    assert_eq!(synth.active_voices(), MAX_POLYPHONY);
    synth.all_notes_off();
    render(&mut synth, 400);
    assert_eq!(synth.active_voices(), 0);
}

#[test]
fn note_off_and_all_notes_off_end_without_a_click() {
    let natural = std::f32::consts::TAU * 220.0 / RATE;
    // The shortest release the envelope allows.
    let mut params = plain(Waveform::Sine);
    params.amp_envelope.release_ms = 1.0;
    let mut synth = synth(&params, RATE);
    synth.note_on(57, 1.0);
    let mut left = render(&mut synth, 4_807).0;
    synth.note_off(57);
    left.extend(render(&mut synth, 2_000).0);
    // The fade itself adds a little speed on top of the tone's own.
    assert!(
        steepest(&left[100..]) < natural + 0.03,
        "{}",
        steepest(&left[100..])
    );
    assert_eq!(synth.active_voices(), 0);

    params.amp_envelope.release_ms = 5_000.0;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(57, 1.0);
    synth.note_on(64, 1.0);
    let mut left = render(&mut synth, 4_811).0;
    synth.note_off(64);
    left.extend(render(&mut synth, 1_000).0);
    synth.all_notes_off();
    let after = render(&mut synth, 1_000).0;
    left.extend(&after);
    assert!(
        steepest(&left[100..]) < 3.0 * natural + 0.01,
        "{}",
        steepest(&left[100..])
    );
    // Gone within the 4 ms fade plus the output filter's delay, whatever
    // the release time says.
    assert!(peak(&after[260..]) < 1.0e-4, "{}", peak(&after[260..]));
    assert_eq!(synth.active_voices(), 0);
}

#[test]
fn glide_slides_the_pitch_from_the_previous_note() {
    let mut params = plain(Waveform::Sine);
    params.glide_ms = 200.0;
    let mut synth = synth(&params, RATE);
    synth.note_on(57, 1.0);
    render(&mut synth, 9_600);
    synth.note_off(57);
    synth.note_on(69, 1.0);
    let latency = synth.latency_samples();
    let (slide, _) = render(&mut synth, 24_000);
    // Halfway through the slide the pitch is halfway between the keys.
    let middle = &slide[latency + 4_800 - 480..latency + 4_800 + 480];
    let halfway = cents(frequency_of(middle, RATE), key_hz(63.0));
    assert!(halfway.abs() < 30.0, "{halfway} cents from the halfway key");
    let arrived = cents(frequency_of(&slide[12_000..], RATE), 440.0);
    assert!(arrived.abs() < 0.5, "{arrived}");

    // Without glide the new note starts in tune.
    params.glide_ms = 0.0;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(57, 1.0);
    render(&mut synth, 9_600);
    synth.note_off(57);
    render(&mut synth, 2_400);
    synth.note_on(69, 1.0);
    let (note, _) = render(&mut synth, 4_800);
    assert!(cents(frequency_of(&note[100..], RATE), 440.0).abs() < 2.0);
}

#[test]
fn mono_plays_one_note_and_falls_back_to_a_held_key() {
    let mut params = plain(Waveform::Sine);
    params.voice_mode = VoiceMode::Mono;
    let mut synth = synth(&params, RATE);
    synth.note_on(57, 1.0);
    render(&mut synth, 4_800);
    synth.note_on(64, 1.0);
    assert_eq!(synth.active_voices(), 1);
    let (second, _) = render(&mut synth, 9_600);
    assert!(cents(frequency_of(&second[2_400..], RATE), key_hz(64.0)).abs() < 1.0);
    assert!(tone_level(&second[1_408..], 220.0, RATE) < 1.0e-3);

    // Letting go of the second key returns to the first, which is still
    // down.
    synth.note_off(64);
    assert_eq!(synth.active_voices(), 1);
    let (back, _) = render(&mut synth, 9_600);
    assert!(cents(frequency_of(&back[2_400..], RATE), 220.0).abs() < 1.0);
    assert!(peak(&back[2_400..]) > 0.95);
    synth.note_off(57);
    render(&mut synth, 1_000);
    assert_eq!(synth.active_voices(), 0);

    // Letting go of a key that is not the sounding one changes nothing.
    synth.note_on(57, 1.0);
    synth.note_on(64, 1.0);
    synth.note_off(57);
    let (still, _) = render(&mut synth, 9_600);
    assert!(cents(frequency_of(&still[2_400..], RATE), key_hz(64.0)).abs() < 1.0);
    synth.note_off(64);
    render(&mut synth, 1_000);
    assert_eq!(synth.active_voices(), 0);
}

#[test]
fn legato_carries_the_envelope_over_and_mono_restarts_it() {
    let level_after_second_note = |voice_mode: VoiceMode| {
        let mut params = plain(Waveform::Sine);
        params.voice_mode = voice_mode;
        params.amp_envelope = EnvelopeParams {
            attack_ms: 5.0,
            decay_ms: 50.0,
            sustain: 0.3,
            release_ms: 50.0,
        };
        let mut synth = synth(&params, RATE);
        // High keys, so that a peak in the level is caught within a cycle.
        synth.note_on(93, 1.0);
        render(&mut synth, 9_600);
        synth.note_on(100, 1.0);
        let (note, _) = render(&mut synth, 1_200);
        (peak(&note), synth.active_voices())
    };
    // Mono restarts the attack, so the level climbs back to full.
    let (mono, voices) = level_after_second_note(VoiceMode::Mono);
    assert!(mono > 0.9, "{mono}");
    assert_eq!(voices, 1);
    // Legato stays at the sustain level.
    let (legato, voices) = level_after_second_note(VoiceMode::Legato);
    assert!((legato - 0.3).abs() < 0.02, "{legato}");
    assert_eq!(voices, 1);
}

#[test]
fn legato_only_glides_between_overlapping_notes() {
    let mut params = plain(Waveform::Sine);
    params.voice_mode = VoiceMode::Legato;
    params.glide_ms = 300.0;
    let mut synth = synth(&params, RATE);
    synth.note_on(57, 1.0);
    render(&mut synth, 4_800);
    synth.note_off(57);
    render(&mut synth, 2_400);
    // Not overlapping: the new note starts on pitch.
    synth.note_on(69, 1.0);
    let (detached, _) = render(&mut synth, 4_800);
    assert!(cents(frequency_of(&detached[200..], RATE), 440.0).abs() < 2.0);
    // Overlapping: the pitch slides.
    synth.note_on(81, 1.0);
    let (tied, _) = render(&mut synth, 4_800);
    let early = cents(frequency_of(&tied[200..2_000], RATE), 880.0);
    assert!(early < -600.0, "{early} cents");
}

#[test]
fn lfos_reach_pitch_volume_cutoff_and_pulse_width() {
    // Vibrato: a square LFO jumps the pitch between a semitone up and a
    // semitone down.
    let mut params = plain(Waveform::Sine);
    params.lfos[0].shape = LfoShape::Square;
    params.lfos[0].rate_hz = 2.0;
    params.lfos[0].pitch_semitones = 1.0;
    let mut synth = synth(&params, RATE);
    synth.note_on(69, 1.0);
    let (note, _) = render(&mut synth, 24_000);
    let up = cents(frequency_of(&note[2_400..9_600], RATE), 440.0);
    let down = cents(frequency_of(&note[14_400..21_600], RATE), 440.0);
    assert!(
        (up - 100.0).abs() < 2.0 && (down + 100.0).abs() < 2.0,
        "{up} {down}"
    );

    // Tremolo: at full depth the volume dips to nothing once per cycle.
    let mut params = plain(Waveform::Sine);
    params.lfos[1].shape = LfoShape::Sine;
    params.lfos[1].rate_hz = 4.0;
    params.lfos[1].amp = 1.0;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(81, 1.0);
    let (note, _) = render(&mut synth, 48_000);
    let levels = envelope(&note[12..], 240);
    let (lowest, highest) = levels
        .iter()
        .fold((f32::MAX, 0.0_f32), |(low, high), level| {
            (low.min(*level), high.max(*level))
        });
    assert!(lowest < 0.02 && highest > 0.98, "{lowest} to {highest}");
    let dips = levels
        .windows(3)
        .filter(|w| w[1] < 0.1 && w[1] <= w[0] && w[1] < w[2])
        .count();
    assert_eq!(dips, 4);

    // Cutoff: with the LFO at its top the filter is open, at its bottom
    // it is closed.
    let mut params = plain(Waveform::Saw);
    params.filter.cutoff_hz = 1_000.0;
    params.lfos[0].shape = LfoShape::Square;
    params.lfos[0].rate_hz = 2.0;
    params.lfos[0].cutoff_octaves = 3.0;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(45, 1.0);
    let (note, _) = render(&mut synth, 24_000);
    let overtone = |window: &[f32]| tone_level(window, 110.0 * 20.0, RATE);
    let open = overtone(&note[2_400..9_600]);
    let closed = overtone(&note[14_400..21_600]);
    assert!(db(open / closed) > 30.0, "{}", db(open / closed));

    // Pulse width: a square is missing its even overtones, and moving the
    // width brings them back.
    let mut params = plain(Waveform::Pulse);
    params.lfos[0].shape = LfoShape::Square;
    params.lfos[0].rate_hz = 2.0;
    params.lfos[0].pulse_width = 0.25;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(45, 1.0);
    let (moved, _) = render(&mut synth, 9_600);
    params.lfos[0].pulse_width = 0.0;
    let mut synth = self::synth(&params, RATE);
    synth.note_on(45, 1.0);
    let (still, _) = render(&mut synth, 9_600);
    assert!(tone_level(&moved[2_400..], 220.0, RATE) > 0.5);
    assert!(tone_level(&still[2_400..], 220.0, RATE) < 0.01);
}

#[test]
fn the_output_lags_by_exactly_the_reported_latency() {
    for rate in RATES {
        let mut synth = synth(&plain(Waveform::Sine), rate);
        let latency = synth.latency_samples();
        synth.note_on(69, 1.0);
        let (note, _) = render(&mut synth, 9_600);
        // The oscillator starts at phase zero. Once the attack is over the
        // output should be that same sine, the latency later. The filter,
        // though wide open at 20 kHz, turns the phase of a 440 Hz tone by
        // a known sliver as any second-order low-pass does.
        // The filter runs at twice the sample rate and maps frequencies
        // onto its response through a tangent.
        let warped = |hz: f64| (std::f64::consts::PI * hz / (2.0 * f64::from(rate))).tan();
        let ratio = warped(440.0) / warped(20_000.0);
        let filter_lag = (std::f64::consts::SQRT_2 * ratio).atan2(1.0 - ratio * ratio);
        let mut worst = 0.0_f64;
        for (index, sample) in note.iter().enumerate().skip(2_400) {
            let seconds = (index - latency) as f64 / f64::from(rate);
            let expected = (std::f64::consts::TAU * 440.0 * seconds - filter_lag).sin();
            worst = worst.max((f64::from(*sample) - expected).abs());
        }
        assert!(worst < 2.0e-3, "{worst} at {rate}");
    }
}

/// A fixed performance: notes, releases and parameter changes at set
/// sample positions.
fn perform(synth: &mut SubtractiveSynth, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut params = SynthParams::default();
    params.oscillators[1].level = 0.6;
    params.oscillators[1].waveform = Waveform::Pulse;
    params.oscillators[1].coarse = -12;
    params.oscillators[2].level = 0.3;
    params.oscillators[2].waveform = Waveform::PinkNoise;
    params.unison_voices = 3;
    params.filter.cutoff_hz = 2_500.0;
    params.filter.resonance = 0.6;
    params.filter.envelope_octaves = 2.5;
    params.filter.slope = FilterSlope::Db24;
    params.filter.drive = 0.4;
    params.lfos[0].pitch_semitones = 0.3;
    params.lfos[1].shape = LfoShape::Random;
    params.lfos[1].cutoff_octaves = 1.0;
    params.lfos[1].pulse_width = 0.2;
    params.glide_ms = 40.0;
    params.polyphony = 3;
    synth.set_params(&params);

    let mut changed = params;
    changed.filter.cutoff_hz = 600.0;
    changed.gain = 0.6;
    changed.pan = -0.4;
    changed.unison_detune_cents = 45.0;
    changed.oscillators[0].waveform = Waveform::Square;

    let events: [(usize, i32, f32); 12] = [
        (0, 60, 1.0),
        (333, 64, 0.7),
        (1_000, 67, 0.9),
        (1_777, -60, 0.0),
        (2_048, 72, 0.5),
        (3_001, 76, 1.0),
        (3_500, -64, 0.0),
        (4_096, 500, 0.0),
        (5_000, -67, 0.0),
        (5_555, 48, 0.8),
        (7_000, 501, 0.0),
        (8_200, 50, 0.6),
    ];
    let frames = 12_000;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut position = 0;
    for (index, (at, what, velocity)) in events.iter().enumerate() {
        debug_assert_eq!(*at, position);
        match *what {
            500 => synth.set_params(&changed),
            501 => synth.all_notes_off(),
            key if key < 0 => synth.note_off(-key as u8),
            key => synth.note_on(key as u8, *velocity),
        }
        let until = events.get(index + 1).map_or(frames, |next| next.0);
        for (left, right) in left[position..until]
            .chunks_mut(block)
            .zip(right[position..until].chunks_mut(block))
        {
            synth.process(left, right);
        }
        position = until;
    }
    (left, right)
}

#[test]
fn block_size_reset_and_repetition_never_change_the_output() {
    let mut reference_synth = synth(&SynthParams::default(), RATE);
    let reference = perform(&mut reference_synth, 12_000);
    assert!(rms(&reference.0) > 0.01 && rms(&reference.1) > 0.01);
    assert!(reference.0 != reference.1);
    for block in [1, 7, 64, 512] {
        let mut other = synth(&SynthParams::default(), RATE);
        assert!(
            perform(&mut other, block) == reference,
            "blocks of {block} differ"
        );
    }
    // A used synth that is reset plays the performance exactly as a new
    // one does.
    let again = perform(&mut reference_synth, 100);
    assert!(
        again != reference,
        "the performance should leave notes ringing"
    );
    reference_synth.reset();
    assert_eq!(reference_synth.active_voices(), 0);
    assert!(perform(&mut reference_synth, 100) == reference);
}

#[test]
fn any_settings_and_any_playing_stay_finite() {
    let mut rng = Rng::new(2_024);
    for round in 0..40 {
        let rate = RATES[round % RATES.len()];
        let params: SynthParams = random_params(&mut rng);
        let mut synth = synth(&params, rate);
        let mut loudest = 0.0_f32;
        for step in 0..120 {
            let roll = rng.unipolar();
            let key = (rng.unipolar() * 140.0) as u8;
            if roll < 0.5 {
                synth.note_on(key, rng.unipolar() * 1.2 - 0.1);
            } else if roll < 0.8 {
                synth.note_off(key);
            } else if roll < 0.85 {
                synth.set_params(&random_params(&mut rng));
            } else if roll < 0.88 {
                synth.all_notes_off();
            } else if roll < 0.9 {
                synth.note_on(key, f32::NAN);
            }
            let block = [1, 16, 100, 256][step % 4];
            let (mut left, mut right) = (vec![f32::NAN; block], vec![f32::NAN; block]);
            synth.process(&mut left, &mut right);
            assert_finite(&left, &format!("round {round}: {params:?}"));
            assert_finite(&right, &format!("round {round}: {params:?}"));
            loudest = loudest.max(peak(&left)).max(peak(&right));
            assert!(synth.active_voices() <= MAX_POLYPHONY + 8);
        }
        // Up to 32 notes of three stacked oscillators through a ringing,
        // overdriven filter at a gain of 2: loud, but bounded.
        assert!(loudest < 2_000.0, "round {round}: {loudest}");
    }
}

#[test]
fn unusual_sample_rates_and_empty_blocks_are_handled() {
    let mut rng = Rng::new(404);
    for rate in [8_000.0, 11_025.0, 22_050.0, 88_200.0, 192_000.0] {
        // Pitch holds at every rate that can carry the note.
        let note = held_note(&plain(Waveform::Sine), 69, rate, 1.0);
        assert!(
            cents(frequency_of(&note, rate), 440.0).abs() < 1.0,
            "A4 at {rate}"
        );

        let params: SynthParams = random_params(&mut rng);
        let mut synth = synth(&params, rate);
        for key in [0, 30, 60, 90, 127] {
            synth.note_on(key, 0.9);
        }
        synth.process(&mut [], &mut []);
        let (left, right) = render(&mut synth, 6_000);
        assert_finite(&left, &format!("{rate}: {params:?}"));
        assert_finite(&right, &format!("{rate}: {params:?}"));
    }
}

#[test]
fn changing_settings_under_a_held_note_never_clicks() {
    let mut params = plain(Waveform::Sine);
    params.filter.cutoff_hz = 4_000.0;
    let mut other = params;
    other.gain = 0.3;
    other.pan = 0.8;
    other.filter.cutoff_hz = 300.0;
    other.filter.resonance = 0.7;
    other.oscillators[0].pan = -0.6;
    other.oscillators[0].fine_cents = 40.0;
    other.amp_velocity = 1.0;
    other.unison_spread = 0.0;
    for rate in RATES {
        let mut synth = synth(&params, rate);
        synth.note_on(57, 0.6);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for step in 0..60 {
            if step % 7 == 3 {
                synth.set_params(if step % 14 == 3 { &other } else { &params });
            }
            let (l, r) = render(&mut synth, 997);
            left.extend(l);
            right.extend(r);
        }
        let natural = std::f32::consts::TAU * 225.0 / rate;
        for output in [&left, &right] {
            let bound = 2.0 * natural * peak(output) + 0.002;
            assert!(
                steepest(&output[500..]) < bound,
                "{} at {rate}",
                steepest(&output[500..])
            );
        }
    }
}

#[test]
fn changing_the_voice_mode_fades_out_what_was_playing() {
    let mut params = plain(Waveform::Sine);
    params.amp_envelope.release_ms = 3_000.0;
    let mut synth = synth(&params, RATE);
    for key in [57, 60, 64] {
        synth.note_on(key, 1.0);
    }
    render(&mut synth, 2_400);
    params.voice_mode = VoiceMode::Mono;
    synth.set_params(&params);
    let (after, _) = render(&mut synth, 1_000);
    assert_eq!(synth.active_voices(), 0);
    assert!(peak(&after[300..]) < 1.0e-4);
    synth.note_on(57, 1.0);
    synth.note_on(60, 1.0);
    assert_eq!(synth.active_voices(), 1);
}

#[test]
fn per_note_project_pan_laws_match_an_independent_stereo_pcm_reference() {
    use windfall_core::PanLaw;
    use windfall_dsp::{NoteExpression, NoteInstanceId};
    let params = plain(Waveform::Sine);
    let mut center = synth(&params, RATE);
    center.note_on_instance(NoteInstanceId(1), 69, 1.0, 0.0, NoteExpression::default());
    let (mono, _) = render(&mut center, 6_000);
    for law in [
        PanLaw::Balance,
        PanLaw::ConstantPower,
        PanLaw::Compromise,
        PanLaw::Linear,
    ] {
        for pan in [-0.6_f32, 0.6] {
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let power = (
                std::f32::consts::SQRT_2 * angle.cos(),
                std::f32::consts::SQRT_2 * angle.sin(),
            );
            let expected = match law {
                PanLaw::Balance => ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0)),
                PanLaw::Linear => (1.0 - pan, 1.0 + pan),
                PanLaw::ConstantPower => power,
                PanLaw::Compromise => (
                    (power.0 * (1.0 - pan)).sqrt(),
                    (power.1 * (1.0 + pan)).sqrt(),
                ),
            };
            let mut unit = synth(&params, RATE);
            unit.set_note_pan_law(law);
            unit.note_on_instance(NoteInstanceId(1), 69, 1.0, pan, NoteExpression::default());
            let (left, right) = render(&mut unit, mono.len());
            for i in 0..mono.len() {
                assert!(
                    (left[i] - mono[i] * expected.0).abs() < 1e-5,
                    "{law:?} left {i}"
                );
                assert!(
                    (right[i] - mono[i] * expected.1).abs() < 1e-5,
                    "{law:?} right {i}"
                );
            }
        }
    }
}

#[test]
fn held_note_law_ramp_has_the_same_pcm_at_every_buffer_size() {
    use windfall_core::PanLaw;
    use windfall_dsp::{NoteExpression, NoteInstanceId};
    let mut reference = None;
    for block in [1, 17, 64, 127, 512] {
        let mut unit = synth(&plain(Waveform::Sine), RATE);
        unit.note_on_instance(NoteInstanceId(1), 69, 1.0, 0.6, NoteExpression::default());
        render(&mut unit, 2_400);
        unit.set_note_pan_law(PanLaw::Linear);
        let mut left = vec![0.0; 1_200];
        let mut right = left.clone();
        for (a, b) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
            unit.process(a, b);
        }
        if let Some((a, b)) = &reference {
            assert_eq!(&left, a);
            assert_eq!(&right, b);
        } else {
            reference = Some((left, right));
        }
    }
}
