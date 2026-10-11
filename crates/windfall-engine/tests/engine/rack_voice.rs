//! Channel rack note tools through the public audio path: note echo of
//! held notes, the sampler channel filter with its envelopes and LFOs, and
//! layer channels.

use windfall_core::{AudioBuffer, TICKS_PER_STEP};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{
    ChannelId, ChannelVoiceSettings, EchoTime, Envelope, LayerChildren, LayerSettings, LfoShape,
    LfoTarget, ModulationEnvelope, NoteDivision, NoteEchoSettings, TargetLfo, VoiceFilter,
    VoiceFilterMode,
};

use crate::realtime::allocator_calls;
use crate::support::{Rig, frequency, left, level, peak, right, rms, run, sine};

const RATE: u32 = 48_000;
/// Frames a cut or stopped voice takes to fade out: 4 ms.
const FADE_FRAMES: usize = 192;
/// Frames of the shortest envelope release: 1 ms.
const RELEASE_FRAMES: usize = 48;

/// An envelope that is fully open while the note is held and closes as
/// fast as an envelope can, so a voice sounds exactly as long as its gate.
fn gate() -> Envelope {
    Envelope {
        attack_ms: 0.0,
        decay_ms: 0.0,
        sustain: 1.0,
        release_ms: 0.0,
    }
}

fn echo(ms: f32, feedback: f32, repeats: u8, pitch_semitones: i8) -> NoteEchoSettings {
    NoteEchoSettings {
        enabled: true,
        time: EchoTime::Milliseconds { ms },
        feedback,
        pitch_semitones,
        repeats,
    }
}

/// A gated sampler channel that echoes its notes.
fn echo_rig(sample: AudioBuffer, settings: NoteEchoSettings) -> (Rig, ChannelId) {
    let mut rig = Rig::new();
    let channel = rig.channel(sample);
    rig.sampler_mut(channel).envelope = Some(gate());
    rig.channel_mut(channel).voice.echo = settings;
    (rig, channel)
}

/// Interleaved samples in one pass of the sixteen-step first pattern, less
/// a frame. Live playback loops the pattern there and a render does not, so
/// the two are compared up to this point.
fn one_pass(rig: &Rig) -> usize {
    let frames = 4.0 * 60.0 / rig.project.settings.tempo_bpm * f64::from(RATE);
    (frames as usize - 1) * 2
}

fn close(actual: f32, expected: f32, tolerance: f32) -> bool {
    (actual - expected).abs() <= tolerance * expected.abs().max(1e-6)
}

// ---------------------------------------------------------------------
// Note echo of notes held by hand

/// Holds a key for `held` frames and returns `total` frames of the left side.
fn hold_and_release(rig: &Rig, channel: ChannelId, held: usize, total: usize, block: usize) -> Vec<f32> {
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let mut out = run(&mut processor, held, block);
    controller.note_off(channel, 60);
    out.extend(run(&mut processor, total - held, block));
    assert_eq!(controller.rejected_echoes(), 0);
    left(&out)
}

#[test]
fn a_held_live_note_echoes_while_held_and_each_echo_lasts_as_long_as_the_note() {
    // 10 ms is 480 frames. The key is held for 1000 frames.
    let (rig, channel) = echo_rig(level(RATE, 0.5, 4.0), echo(10.0, 0.5, 2, 0));
    let out = hold_and_release(&rig, channel, 1000, 3000, 64);
    let dry = out[100];
    assert!(dry > 0.0);
    // The first echo starts while the key is still down, at half velocity,
    // and the second at a quarter.
    assert!(close(out[400], dry, 1e-5), "nothing echoes before 10 ms");
    assert!(close(out[600], dry * 1.5, 1e-5));
    assert!(close(out[990], dry * 1.75, 1e-5));
    // The key goes up on frame 1000. Each echo was held as long as the
    // note: the first ends on frame 1480 and the second on frame 1960.
    assert!(close(out[1000 + RELEASE_FRAMES + 10], dry * 0.75, 1e-5));
    assert!(close(out[1470], dry * 0.75, 1e-5));
    assert!(close(out[1480 + RELEASE_FRAMES + 10], dry * 0.25, 1e-5));
    assert!(close(out[1950], dry * 0.25, 1e-5));
    assert_eq!(peak(&out[1960 + RELEASE_FRAMES + 1..]), 0.0, "no echo is left held");
}

#[test]
fn held_echo_does_not_depend_on_how_the_output_is_cut_into_blocks() {
    let (rig, channel) = echo_rig(sine(RATE, 330.0, 4.0), echo(7.3, 0.7, 4, 3));
    let reference = hold_and_release(&rig, channel, 1000, 4000, 1000);
    assert!(peak(&reference[1200..]) > 0.0);
    for block in [1, 7, 64, 250, 500] {
        let other = hold_and_release(&rig, channel, 1000, 4000, block);
        assert!(
            reference
                .iter()
                .zip(&other)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "block size {block} changed the echoes"
        );
    }
}

#[test]
fn held_echo_repeats_are_transposed_and_fade_by_the_feedback() {
    // One repeat an octave up, 100 ms (4800 frames) after the note.
    let (rig, channel) = echo_rig(sine(RATE, 440.0, 4.0), echo(100.0, 0.5, 1, 12));
    let out = hold_and_release(&rig, channel, 2400, 9600, 256);
    let dry = &out[240..2160];
    let repeat = &out[5040..6960];
    assert!((frequency(dry, RATE) - 440.0).abs() < 0.5);
    assert!((frequency(repeat, RATE) - 880.0).abs() < 1.0);
    assert!(close(rms(repeat), rms(dry) * 0.5, 0.01));
    assert_eq!(peak(&out[7200 + RELEASE_FRAMES + 1..]), 0.0);
}

#[test]
fn a_key_released_before_its_first_echo_still_gets_whole_echoes() {
    // Held for 100 frames; the echoes come 480 and 960 frames later.
    let (rig, channel) = echo_rig(level(RATE, 0.5, 4.0), echo(10.0, 0.5, 2, 0));
    let out = hold_and_release(&rig, channel, 100, 2000, 100);
    let dry = out[50];
    assert_eq!(out[300], 0.0, "between the note and its first echo");
    assert!(close(out[530], dry * 0.5, 1e-5));
    assert_eq!(out[580 + RELEASE_FRAMES + 20], 0.0);
    assert!(close(out[1010], dry * 0.25, 1e-5));
    assert_eq!(peak(&out[1060 + RELEASE_FRAMES + 1..]), 0.0);
}

#[test]
fn tempo_synced_held_echo_follows_the_project_tempo() {
    // A sixteenth at 150 BPM is 0.1 s: 4800 frames.
    let settings = NoteEchoSettings {
        time: EchoTime::Division {
            division: NoteDivision::Sixteenth,
        },
        ..echo(1.0, 0.5, 1, 0)
    };
    let (mut rig, channel) = echo_rig(level(RATE, 0.5, 4.0), settings);
    rig.project.settings.tempo_bpm = 150.0;
    let out = hold_and_release(&rig, channel, 200, 9600, 200);
    let dry = out[100];
    assert_eq!(peak(&out[200 + RELEASE_FRAMES + 1..4800]), 0.0);
    assert!(close(out[4800 + 100], dry * 0.5, 1e-5));
    assert_eq!(peak(&out[5000 + RELEASE_FRAMES + 1..]), 0.0);
}

#[test]
fn hardware_notes_echo_like_live_notes_and_panic_cuts_their_echoes() {
    let (rig, channel) = echo_rig(level(RATE, 0.5, 4.0), echo(10.0, 0.5, 2, 0));
    let (mut processor, controller) = rig.processor(RATE);
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 60, 127));
    let held = left(&run(&mut processor, 1000, 64));
    let dry = held[100];
    assert!(close(held[990], dry * 1.75, 1e-5));
    assert!(controller.hardware_note(epoch, channel, 60, 0));
    let after = left(&run(&mut processor, 3000, 64));
    assert!(close(after[400], dry * 0.75, 1e-5));
    assert_eq!(peak(&after[960 + RELEASE_FRAMES + 1..]), 0.0);

    // A panic while echoes are queued and sounding silences all of them.
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 60, 127));
    run(&mut processor, 600, 64);
    controller.panic_hardware();
    let out = left(&run(&mut processor, 4000, 64));
    assert_eq!(peak(&out[FADE_FRAMES + 1..]), 0.0);
}

#[test]
fn stopping_the_transport_cuts_queued_and_sounding_echoes() {
    // Sequenced: a one-step note with three 100 ms echoes.
    let (mut rig, channel) = echo_rig(level(RATE, 0.5, 4.0), echo(100.0, 0.9, 3, 0));
    rig.note(channel, 0, TICKS_PER_STEP);
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let playing = left(&run(&mut processor, 6000, 128));
    assert!(playing[5000] > 0.0, "the first echo sounds");
    controller.stop();
    let stopped = left(&run(&mut processor, 24_000, 128));
    assert_eq!(peak(&stopped[FADE_FRAMES + 1..]), 0.0);

    // Held by hand: stop silences the note and none of its echoes follow.
    controller.note_on(channel, 60, 1.0);
    run(&mut processor, 600, 128);
    controller.stop();
    let stopped = left(&run(&mut processor, 24_000, 128));
    assert_eq!(peak(&stopped[FADE_FRAMES + 1..]), 0.0);
    controller.note_off(channel, 60);
    assert_eq!(peak(&run(&mut processor, 24_000, 128)), 0.0);
}

#[test]
fn echoes_that_do_not_fit_are_counted_and_their_notes_stay_dry() {
    // Eight repeats take sixteen queue slots a note, so sixteen held notes
    // fill the 256-slot queue and every later one plays dry.
    let (rig, channel) = echo_rig(level(RATE, 0.01, 4.0), echo(50.0, 0.95, 8, 0));
    let (mut processor, controller) = rig.processor(RATE);
    for key in 40..64 {
        controller.note_on(channel, key, 1.0);
    }
    let held = run(&mut processor, 256, 64);
    assert!(peak(&held) > 0.0);
    assert_eq!(controller.rejected_echoes(), 8);
    for key in 40..64 {
        controller.note_off(channel, key);
    }
    // Eight repeats of 50 ms end 400 ms after the release.
    let out = left(&run(&mut processor, 24_000, 256));
    assert!(peak(&out[..19_200]) > 0.0);
    assert_eq!(peak(&out[19_200 + RELEASE_FRAMES + 1..]), 0.0, "no note is stuck");
    assert_eq!(controller.frame().voices, 0);
}

#[test]
fn sequenced_echo_sounds_the_same_live_and_offline() {
    let (mut rig, channel) = echo_rig(sine(RATE, 220.0, 4.0), echo(31.0, 0.6, 3, 7));
    rig.steps(channel, &[0, 3, 6]);
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 0.5,
        block_frames: 333,
        ..RenderOptions::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let live = rig.play(RATE, rendered.frames(), 64);
    let pass = one_pass(&rig);
    assert!(peak(&live[..pass]) > 0.0);
    assert!(
        live[..pass]
            .iter()
            .zip(rendered.samples())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn held_echo_never_allocates_or_frees_on_the_audio_path() {
    let (rig, channel) = echo_rig(sine(RATE, 330.0, 1.0), echo(5.0, 0.9, 8, 1));
    let (mut processor, controller) = rig.processor(RATE);
    let mut out = vec![0.0; 512 * 2];
    for round in 0..6 {
        // More keys than the echo queue has room for, so refusals run too.
        for key in 30..70 {
            controller.note_on(channel, key, 0.5);
        }
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        for key in 30..70 {
            controller.note_off(channel, key);
        }
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        if round % 2 == 1 {
            controller.stop();
        }
        for _ in 0..8 {
            assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        }
        controller.frame();
    }
    assert!(controller.rejected_echoes() > 0);
}

// ---------------------------------------------------------------------
// The sampler channel filter, its envelopes and its LFOs

/// The level of a held tone of `frequency` hertz after the first 100 ms,
/// through a sampler channel with these voice settings.
fn tone_level(frequency: f64, voice: ChannelVoiceSettings) -> f32 {
    let mut rig = Rig::new();
    let channel = rig.channel(sine(RATE, frequency, 2.0));
    rig.channel_mut(channel).voice = voice;
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let out = left(&run(&mut processor, 14_400, 256));
    rms(&out[4800..])
}

fn filtered(mode: VoiceFilterMode, cutoff_hz: f32, resonance: f32) -> ChannelVoiceSettings {
    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.filter_settings = VoiceFilter {
        enabled: true,
        mode,
        cutoff_hz,
        resonance,
    };
    voice
}

fn decibels(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

#[test]
fn the_channel_filter_passes_and_rejects_by_its_mode() {
    let dry = |frequency| tone_level(frequency, ChannelVoiceSettings::default());
    let through = |frequency, mode| {
        decibels(tone_level(frequency, filtered(mode, 1000.0, 0.0)) / dry(frequency))
    };
    // 12 dB an octave: three octaves away is about 36 dB down.
    assert!(through(125.0, VoiceFilterMode::LowPass).abs() < 0.2);
    assert!(through(8000.0, VoiceFilterMode::LowPass) < -33.0);
    assert!(through(8000.0, VoiceFilterMode::HighPass).abs() < 0.2);
    assert!(through(125.0, VoiceFilterMode::HighPass) < -33.0);
    assert!(through(1000.0, VoiceFilterMode::BandPass).abs() < 0.3);
    assert!(through(125.0, VoiceFilterMode::BandPass) < -12.0);
    assert!(through(8000.0, VoiceFilterMode::BandPass) < -12.0);
    assert!(through(1000.0, VoiceFilterMode::Notch) < -30.0);
    assert!(through(125.0, VoiceFilterMode::Notch).abs() < 0.5);
    assert!(through(8000.0, VoiceFilterMode::Notch).abs() < 0.5);
}

#[test]
fn a_filter_that_is_switched_off_changes_nothing_whatever_its_settings() {
    let mut voice = filtered(VoiceFilterMode::HighPass, 9000.0, 1.0);
    voice.envelopes.filter_settings.enabled = false;
    assert_eq!(
        tone_level(440.0, voice).to_bits(),
        tone_level(440.0, ChannelVoiceSettings::default()).to_bits()
    );
}

#[test]
fn resonance_raises_the_peak_at_the_cutoff() {
    let flat = tone_level(1000.0, filtered(VoiceFilterMode::LowPass, 1000.0, 0.0));
    let sharp = tone_level(1000.0, filtered(VoiceFilterMode::LowPass, 1000.0, 0.6));
    // A low-pass passes its cutoff at a gain of Q, and Q doubles with every
    // fifth of the resonance range: 0.6 is three octaves, eight times.
    assert!(close(sharp / flat, 8.0, 0.03), "{}", sharp / flat);
    // Away from the cutoff the resonance leaves the level alone.
    let low = tone_level(100.0, filtered(VoiceFilterMode::LowPass, 1000.0, 0.6));
    assert!(close(low, tone_level(100.0, ChannelVoiceSettings::default()), 0.02));
}

fn held(depth: f32) -> ModulationEnvelope {
    ModulationEnvelope {
        enabled: true,
        attack_ms: 0.0,
        decay_ms: 1.0,
        sustain: 1.0,
        release_ms: 1.0,
        depth,
    }
}

#[test]
fn the_cutoff_envelope_and_lfo_move_the_first_modulation_target() {
    // A 4 kHz tone through a 250 Hz low-pass is four octaves above the
    // cutoff. Raising the cutoff five octaves lets it through.
    let closed = tone_level(4000.0, filtered(VoiceFilterMode::LowPass, 250.0, 0.0));
    let mut opened = filtered(VoiceFilterMode::LowPass, 250.0, 0.0);
    opened.envelopes.filter = held(5.0);
    let open = tone_level(4000.0, opened);
    let dry = tone_level(4000.0, ChannelVoiceSettings::default());
    assert!(decibels(closed / dry) < -45.0);
    assert!(decibels(open / dry).abs() < 1.5);

    // A slow attack opens the filter gradually.
    let mut rig = Rig::new();
    let channel = rig.channel(sine(RATE, 4000.0, 2.0));
    let mut sweep = filtered(VoiceFilterMode::LowPass, 250.0, 0.0);
    sweep.envelopes.filter = ModulationEnvelope {
        attack_ms: 100.0,
        ..held(5.0)
    };
    rig.channel_mut(channel).voice = sweep;
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let out = left(&run(&mut processor, 9600, 256));
    let early = rms(&out[240..720]);
    let middle = rms(&out[2400..2880]);
    let late = rms(&out[7200..9600]);
    assert!(early < middle && middle < late);
    assert!(decibels(late / early) > 30.0);

    // The cutoff LFO, as a square wave, holds the filter open for the first
    // half of each cycle and shut for the second.
    let mut rig = Rig::new();
    let channel = rig.channel(sine(RATE, 4000.0, 2.0));
    let mut wobble = filtered(VoiceFilterMode::LowPass, 1000.0, 0.0);
    wobble.envelopes.lfos.cutoff = TargetLfo {
        enabled: true,
        shape: LfoShape::Square,
        rate_hz: 5.0,
        depth: 0.5,
    };
    rig.channel_mut(channel).voice = wobble;
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let out = left(&run(&mut processor, 9600, 256));
    // 5 Hz is 9600 frames a cycle: four octaves up, then four down.
    let up = rms(&out[2400..4560]);
    let down = rms(&out[7200..9360]);
    assert!(decibels(up / dry).abs() < 1.0);
    assert!(decibels(down / dry) < -60.0);
}

#[test]
fn the_resonance_envelope_and_lfo_move_the_second_modulation_target() {
    let fixed = tone_level(1000.0, filtered(VoiceFilterMode::LowPass, 1000.0, 0.6));
    let mut by_envelope = filtered(VoiceFilterMode::LowPass, 1000.0, 0.0);
    by_envelope.envelopes.resonance = held(0.6);
    assert!(close(tone_level(1000.0, by_envelope), fixed, 0.02));

    // A square resonance LFO: sharp for half a cycle, flat for the other.
    let mut rig = Rig::new();
    let channel = rig.channel(sine(RATE, 1000.0, 2.0));
    let mut voice = filtered(VoiceFilterMode::LowPass, 1000.0, 0.0);
    voice.envelopes.lfos.resonance = TargetLfo {
        enabled: true,
        shape: LfoShape::Square,
        rate_hz: 5.0,
        depth: 0.6,
    };
    rig.channel_mut(channel).voice = voice;
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let out = left(&run(&mut processor, 9600, 256));
    let flat = tone_level(1000.0, filtered(VoiceFilterMode::LowPass, 1000.0, 0.0));
    assert!(close(rms(&out[2400..4560]), fixed, 0.03));
    assert!(close(rms(&out[7200..9360]), flat, 0.03));
}

/// 9600 frames of a held constant level through these voice settings.
fn held_level(voice: ChannelVoiceSettings) -> Vec<f32> {
    let mut rig = Rig::new();
    let channel = rig.channel(level(RATE, 0.5, 2.0));
    rig.channel_mut(channel).voice = voice;
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    run(&mut processor, 9600, 256)
}

#[test]
fn the_volume_lfo_is_a_tremolo_between_its_depth_and_full_level() {
    let dry = left(&held_level(ChannelVoiceSettings::default()))[100];
    let tremolo = |shape, depth| {
        let mut voice = ChannelVoiceSettings::default();
        voice.envelopes.lfos.volume = TargetLfo {
            enabled: true,
            shape,
            rate_hz: 10.0,
            depth,
        };
        left(&held_level(voice))
    };
    // 10 Hz is 4800 frames a cycle. A sine starts halfway, peaks a quarter
    // of the way in and is silent at three quarters.
    let full = tremolo(LfoShape::Sine, 1.0);
    assert!(close(full[0], dry * 0.5, 1e-4));
    assert!(close(full[1200], dry, 1e-4));
    assert!(full[3600].abs() < dry * 1e-4);
    assert!(close(full[4800 + 1200], dry, 1e-4), "the cycle repeats");
    assert!(full.iter().all(|sample| *sample <= dry * 1.000_01));
    // Half depth swings between half and full level.
    let half = tremolo(LfoShape::Square, 0.5);
    assert!(close(half[100], dry, 1e-5));
    assert!(close(half[2500], dry * 0.5, 1e-5));
    // A negative depth is the same swing the other way round.
    let flipped = tremolo(LfoShape::Square, -0.5);
    assert!(close(flipped[100], dry * 0.5, 1e-5));
    assert!(close(flipped[2500], dry, 1e-5));

    // The assignable LFO reaches the same target.
    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.lfo.enabled = true;
    voice.envelopes.lfo.target = LfoTarget::Volume;
    voice.envelopes.lfo.shape = LfoShape::Square;
    voice.envelopes.lfo.rate_hz = 10.0;
    voice.envelopes.lfo.depth = 0.5;
    let assigned = left(&held_level(voice));
    assert_eq!(assigned, half);
}

#[test]
fn the_pan_lfo_and_envelope_move_the_note_between_the_sides() {
    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.lfos.pan = TargetLfo {
        enabled: true,
        shape: LfoShape::Square,
        rate_hz: 10.0,
        depth: 1.0,
    };
    let out = held_level(voice);
    let (l, r) = (left(&out), right(&out));
    // Hard right for the first half cycle, hard left for the second.
    assert!(l[1000].abs() < 1e-6 && r[1000] > 0.1);
    assert!(r[3500].abs() < 1e-6 && l[3500] > 0.1);

    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.pan = held(-1.0);
    let out = held_level(voice);
    assert!(right(&out)[2000].abs() < 1e-6 && left(&out)[2000] > 0.1);
}

#[test]
fn the_pitch_envelope_and_lfo_bend_the_note() {
    let bent = |voice: ChannelVoiceSettings| {
        let mut rig = Rig::new();
        let channel = rig.channel(sine(RATE, 440.0, 2.0));
        rig.channel_mut(channel).voice = voice;
        let (mut processor, controller) = rig.processor(RATE);
        controller.note_on(channel, 60, 1.0);
        left(&run(&mut processor, 19_200, 256))
    };
    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.pitch = held(1200.0);
    let up = bent(voice);
    assert!((frequency(&up[2400..9600], RATE) - 880.0).abs() < 0.5);

    // A square pitch LFO at half depth: an octave up, then an octave down.
    let mut voice = ChannelVoiceSettings::default();
    voice.envelopes.lfos.pitch = TargetLfo {
        enabled: true,
        shape: LfoShape::Square,
        rate_hz: 5.0,
        depth: 0.5,
    };
    let wobble = bent(voice);
    assert!((frequency(&wobble[480..4320], RATE) - 880.0).abs() < 1.0);
    assert!((frequency(&wobble[5280..9120], RATE) - 220.0).abs() < 1.0);
}

/// Every envelope, LFO and the filter at once, on notes of a pattern.
fn modulated_rig() -> Rig {
    let mut rig = Rig::new();
    let channel = rig.channel(sine(RATE, 330.0, 2.0));
    rig.sampler_mut(channel).envelope = Some(Envelope {
        attack_ms: 2.0,
        decay_ms: 40.0,
        sustain: 0.6,
        release_ms: 30.0,
    });
    let voice = &mut rig.channel_mut(channel).voice;
    voice.envelopes.filter_settings = VoiceFilter {
        enabled: true,
        mode: VoiceFilterMode::BandPass,
        cutoff_hz: 700.0,
        resonance: 0.4,
    };
    voice.envelopes.filter = ModulationEnvelope {
        attack_ms: 12.0,
        decay_ms: 60.0,
        sustain: 0.3,
        release_ms: 80.0,
        ..held(2.5)
    };
    voice.envelopes.resonance = ModulationEnvelope {
        attack_ms: 30.0,
        ..held(0.3)
    };
    voice.envelopes.pitch = ModulationEnvelope {
        attack_ms: 0.0,
        decay_ms: 25.0,
        sustain: 0.0,
        ..held(700.0)
    };
    voice.envelopes.pan = ModulationEnvelope {
        attack_ms: 50.0,
        ..held(0.5)
    };
    let lfo = |rate_hz, depth, shape| TargetLfo {
        enabled: true,
        shape,
        rate_hz,
        depth,
    };
    voice.envelopes.lfos.volume = lfo(7.0, 0.4, LfoShape::Sine);
    voice.envelopes.lfos.pan = lfo(3.0, -0.3, LfoShape::Triangle);
    voice.envelopes.lfos.pitch = lfo(5.5, 0.01, LfoShape::Sine);
    voice.envelopes.lfos.cutoff = lfo(2.0, 0.2, LfoShape::Triangle);
    voice.envelopes.lfos.resonance = lfo(1.5, 0.2, LfoShape::Square);
    voice.envelopes.lfo.enabled = true;
    voice.envelopes.lfo.target = LfoTarget::Resonance;
    voice.envelopes.lfo.depth = 0.1;
    rig.steps(channel, &[0, 2, 5, 9]);
    rig.note(channel, 12 * TICKS_PER_STEP, 3 * TICKS_PER_STEP).key = 67;
    rig
}

#[test]
fn modulation_is_the_same_for_every_block_size_and_offline() {
    let rig = modulated_rig();
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 0.25,
        block_frames: 480,
        ..RenderOptions::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(peak(rendered.samples()) > 0.01);
    assert!(rendered.samples().iter().all(|sample| sample.is_finite()));
    let pass = one_pass(&rig);
    for block in [1, 17, 256, 1024] {
        let live = rig.play(RATE, rendered.frames(), block);
        assert!(
            live[..pass]
                .iter()
                .zip(rendered.samples())
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "block size {block} differs from the offline render"
        );
    }
}

#[test]
fn modulation_never_allocates_or_frees_on_the_audio_path() {
    let rig = modulated_rig();
    let channel = rig.project.channels[0].id;
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut out = vec![0.0; 512 * 2];
    for round in 0..40 {
        if round % 5 == 0 {
            controller.note_on(channel, 48 + round as u8, 0.7);
        }
        if round % 5 == 3 {
            controller.note_off(channel, 45 + round as u8);
        }
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    }
    assert!(peak(&out) > 0.0);
    controller.stop();
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
}

// ---------------------------------------------------------------------
// Layer channels

fn layer(children: &[ChannelId]) -> LayerSettings {
    LayerSettings {
        enabled: true,
        children: LayerChildren::new(children.iter().copied()),
    }
}

/// Two sounding channels on tracks of their own and a layer over them. The
/// layer has a loud sample of its own, which must never be heard.
fn layered() -> (Rig, ChannelId, ChannelId, ChannelId) {
    let mut rig = Rig::new();
    let first = rig.track();
    let second = rig.track();
    let low = rig.channel_on(sine(RATE, 220.0, 1.0), first);
    let high = rig.channel_on(sine(RATE, 3300.0, 1.0), second);
    rig.sampler_mut(low).envelope = Some(gate());
    rig.channel_mut(high).pan = 0.5;
    let top = rig.channel(level(RATE, 1.0, 1.0));
    rig.channel_mut(top).voice.layer = layer(&[low, high]);
    (rig, top, low, high)
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}

#[test]
fn a_layer_plays_its_notes_on_every_child_and_makes_no_sound_itself() {
    let (mut layered_rig, top, low, high) = layered();
    layered_rig.steps(top, &[0, 4]);
    layered_rig.note(top, 8 * TICKS_PER_STEP, 2 * TICKS_PER_STEP).key = 67;
    let through_layer = layered_rig.play(RATE, 48_000, 256);

    // The same notes written on the two children directly.
    let (mut direct_rig, _, _, _) = layered();
    for channel in [low, high] {
        direct_rig.steps(channel, &[0, 4]);
        direct_rig.note(channel, 8 * TICKS_PER_STEP, 2 * TICKS_PER_STEP).key = 67;
    }
    let direct = direct_rig.play(RATE, 48_000, 256);
    assert!(peak(&direct) > 0.1);
    assert!(same_bits(&through_layer, &direct));

    // A layer with no children, or one that is switched off and has no
    // sample, is silent; switched off with a sample it is a plain sampler.
    let (mut empty, top, _, _) = layered();
    empty.channel_mut(top).voice.layer = layer(&[]);
    empty.steps(top, &[0, 4]);
    assert_eq!(peak(&empty.play(RATE, 24_000, 256)), 0.0);
    empty.channel_mut(top).voice.layer.enabled = false;
    assert!(peak(&empty.play(RATE, 24_000, 256)) > 0.5);
}

#[test]
fn nested_layers_reach_each_sounding_channel_once() {
    let (mut rig, top, low, high) = layered();
    // top -> [inner, low], inner -> [low, high]: low is reached twice.
    let inner = rig.channel(level(RATE, 1.0, 1.0));
    rig.channel_mut(inner).voice.layer = layer(&[low, high]);
    rig.channel_mut(top).voice.layer = layer(&[inner, low]);
    rig.steps(top, &[0, 4]);
    let nested = rig.play(RATE, 24_000, 256);

    let (mut direct, _, _, _) = layered();
    for channel in [low, high] {
        direct.steps(channel, &[0, 4]);
    }
    let direct = direct.play(RATE, 24_000, 256);
    assert!(peak(&direct) > 0.1);
    assert!(same_bits(&nested, &direct));
}

#[test]
fn live_layer_notes_start_and_release_on_every_child() {
    let (rig, top, low, high) = layered();
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(top, 64, 0.8);
    let mut through_layer = run(&mut processor, 4800, 128);
    controller.note_off(top, 64);
    through_layer.extend(run(&mut processor, 4800, 128));

    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(low, 64, 0.8);
    controller.note_on(high, 64, 0.8);
    let mut direct = run(&mut processor, 4800, 128);
    controller.note_off(low, 64);
    controller.note_off(high, 64);
    direct.extend(run(&mut processor, 4800, 128));

    assert!(peak(&direct[..9600]) > 0.1);
    assert!(same_bits(&through_layer, &direct));
    // The gated child stopped with the key.
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(low, 64, 0.8);
    run(&mut processor, 4800, 128);
    controller.note_off(low, 64);
    let released = run(&mut processor, 4800, 128);
    assert_eq!(peak(&released[(RELEASE_FRAMES + 1) * 2..]), 0.0);
}

#[test]
fn mute_and_solo_treat_a_layer_as_the_source_of_its_childrens_notes() {
    let (mut rig, top, low, high) = layered();
    rig.steps(top, &[0, 4]);
    let all = rig.play(RATE, 24_000, 256);
    assert!(peak(&all) > 0.1);

    // A muted layer passes nothing on.
    rig.channel_mut(top).muted = true;
    assert_eq!(peak(&rig.play(RATE, 24_000, 256)), 0.0);
    rig.channel_mut(top).muted = false;

    // A muted child is left out; the other still plays.
    rig.channel_mut(high).muted = true;
    let (mut only_low, _, _, _) = layered();
    only_low.steps(low, &[0, 4]);
    assert!(same_bits(
        &rig.play(RATE, 24_000, 256),
        &only_low.play(RATE, 24_000, 256)
    ));
    rig.channel_mut(high).muted = false;

    // Soloing the layer keeps its children audible and silences the rest.
    let other = rig.channel(level(RATE, 0.9, 1.0));
    rig.steps(other, &[0, 4]);
    rig.channel_mut(top).solo = true;
    assert!(same_bits(&rig.play(RATE, 24_000, 256), &all));
    // Soloing another channel silences the layer's notes.
    rig.channel_mut(top).solo = false;
    rig.channel_mut(other).solo = true;
    let (mut only_other, _, _, _) = layered();
    let alone = only_other.channel(level(RATE, 0.9, 1.0));
    only_other.steps(alone, &[0, 4]);
    assert!(same_bits(
        &rig.play(RATE, 24_000, 256),
        &only_other.play(RATE, 24_000, 256)
    ));
}

#[test]
fn layers_sound_the_same_offline_and_never_allocate_on_the_audio_path() {
    let (mut rig, top, low, _) = layered();
    rig.channel_mut(low).voice.echo = echo(20.0, 0.5, 2, 12);
    rig.steps(top, &[0, 3, 6, 9]);
    let options = RenderOptions {
        sample_rate: RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 0.25,
        block_frames: 777,
        ..RenderOptions::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let live = rig.play(RATE, rendered.frames(), 96);
    let pass = one_pass(&rig);
    assert!(peak(&live[..pass]) > 0.1);
    assert!(same_bits(&live[..pass], &rendered.samples()[..pass]));

    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut out = vec![0.0; 512 * 2];
    for round in 0..30 {
        if round % 4 == 0 {
            controller.note_on(top, 50 + round as u8, 0.6);
        }
        if round % 4 == 2 {
            controller.note_off(top, 48 + round as u8);
        }
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    }
    // Removing a child while the layer plays swaps plans without allocating.
    let (edited, _, _, _) = {
        let (mut edited, top, low, high) = layered();
        edited.channel_mut(top).voice.layer = layer(&[low]);
        (edited, top, low, high)
    };
    controller.set_project(&edited.project, &edited.pool);
    for _ in 0..8 {
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    }
    controller.frame();
}
