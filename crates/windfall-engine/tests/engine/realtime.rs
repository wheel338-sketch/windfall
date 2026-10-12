//! The audio path must never touch the allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use windfall_core::TICKS_PER_STEP;
use windfall_dsp::{
    CompressorParams, DelayParams, EffectKind, EqParams, LimiterParams, ReverbParams,
};
use windfall_engine::SamplePool;
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{
    AutomationId, ChannelId, ClipContent, ClipId, EffectId, EffectParams, Envelope, Send, TrackId,
};

use crate::support::{
    Rig, automate_everything, idle_limiter, impulse, level, limiter, peak, plain_synth, sine,
};

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every call made on a thread while that
/// thread is inside [`allocator_calls`].
pub struct CountingAllocator;

fn count() {
    // The thread-locals hold plain values with no destructor, so reading
    // them here cannot allocate. `try_with` covers a thread being torn down.
    if WATCHING.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}

// SAFETY: every method forwards to the system allocator unchanged. Counting
// reads and writes thread-local cells and nothing else.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

/// Runs `work` and returns how many times it allocated, reallocated or
/// freed memory.
pub(super) fn allocator_calls(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    WATCHING.set(true);
    work();
    WATCHING.set(false);
    CALLS.get()
}

#[test]
fn the_counter_sees_allocations_and_frees() {
    let mut kept = None;
    assert_eq!(allocator_calls(|| kept = Some(vec![1_u8; 64])), 1);
    assert_eq!(allocator_calls(|| drop(kept.take())), 1);
    assert_eq!(allocator_calls(|| assert_eq!(2 + 2, 4)), 0);
}

#[test]
fn sampler_loops_never_allocate_or_free_during_notes_cuts_edits_and_retirement() {
    for mode in [
        windfall_project::SamplerLoopMode::Forward,
        windfall_project::SamplerLoopMode::PingPong,
    ] {
        let mut rig = Rig::new();
        let channel = rig.channel(sine(44_100, 220.0, 0.001));
        rig.sampler_mut(channel).loop_mode = mode;
        let (mut processor, controller) = rig.processor(48_000);
        let mut out = vec![0.0; 512 * 2];
        for round in 0..8 {
            for note in 0..320 {
                controller.note_on(channel, (note % 128) as u8, 0.001);
            }
            assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
            for key in 0..128 {
                controller.note_off(channel, key);
            }
            assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
            let sampler = rig.sampler_mut(channel);
            sampler.loop_start = round as f32 * 0.05;
            sampler.loop_end = 1.0 - round as f32 * 0.05;
            sampler.cut_self = true;
            if round == 4 {
                let sample = sampler.sample.unwrap();
                rig.pool.insert(sample, sine(32_000, 440.0, 0.002));
            }
            controller.set_project(&rig.project, &rig.pool);
            assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
            controller.frame();
        }
        controller.stop();
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        controller.frame();
    }
}

/// A project that keeps every part of the engine busy: resampled and
/// pitched voices, envelopes, cut groups, sends, swing and a playlist.
fn busy_rig() -> Rig {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 174.0;
    rig.project.settings.swing = 0.4;
    let drums = rig.track();
    let effects = rig.track();
    rig.track_mut(drums).sends.push(Send {
        target: effects,
        gain: 0.4,
    });

    let kick = rig.channel_on(sine(44_100, 60.0, 0.3), drums);
    rig.steps(kick, &[0, 4, 8, 12]);
    let hat = rig.channel_on(sine(48_000, 7_000.0, 0.2), drums);
    rig.sampler_mut(hat).cut_self = true;
    rig.sampler_mut(hat).cut_group = 1;
    rig.steps(hat, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let open_hat = rig.channel_on(sine(32_000, 5_000.0, 0.6), drums);
    rig.sampler_mut(open_hat).cut_group = 1;
    rig.steps(open_hat, &[2, 10]);
    let pad = rig.channel_on(sine(44_100, 220.0, 2.0), effects);
    rig.sampler_mut(pad).envelope = Some(Envelope {
        attack_ms: 20.0,
        decay_ms: 100.0,
        sustain: 0.5,
        release_ms: 200.0,
    });
    for (index, key) in [57, 60, 64, 67].into_iter().enumerate() {
        rig.note(pad, index as u32 * 960, 900).key = key;
    }
    let click = rig.channel(impulse(48_000));
    rig.steps(click, &[0, 8]);
    // More notes on one tick than the sequencer hands over in one round,
    // half of them silent.
    let crowd = rig.channel_on(impulse(48_000), drums);
    for index in 0..1_100 {
        rig.note(crowd, 1_000, 60).velocity = (index % 2) as f32 * 0.01;
    }

    let second = rig.pattern(8);
    rig.note_in(second, kick, 0, TICKS_PER_STEP);
    rig.note_in(second, pad, 480, 2_000).key = 72;
    let lane = rig.playlist_track();
    let first = rig.first_pattern();
    rig.clip(lane, first, 0, 3_840 * 2);
    rig.clip(lane, second, 1_920, 5_000).offset = 300;

    // Audio clips: pitched and faded on a track with a send, reversed on
    // another, and more at once on one tick than there are slots to play
    // them in.
    let tape = rig.playlist_track();
    let riser = rig.audio_clip(tape, sine(44_100, 500.0, 1.5), drums, 500, 6_000);
    rig.audio_mut(riser, |audio| {
        audio.pitch = 2.0;
        audio.fade_in = 400;
        audio.fade_out = 900;
        audio.pan = 0.4;
    });
    rig.clip_mut(riser).offset = 130;
    let reversed = rig.audio_clip(tape, sine(48_000, 300.0, 0.4), effects, 2_000, 900);
    rig.audio_mut(reversed, |audio| audio.reverse = true);
    let crowd = rig.playlist_track();
    for _ in 0..140 {
        rig.audio_clip(crowd, level(48_000, 0.001, 0.05), drums, 3_000, 200);
    }
    rig
}

/// The busy project with a third mixer track that has two effects on it, a
/// synth, and an automation of every kind of target on its playlist.
fn automated_rig() -> (Rig, ChannelId) {
    let mut rig = busy_rig();
    let keys = rig.track();
    rig.effect(keys, effect_at(EffectKind::Delay, 0));
    rig.effect(keys, effect_at(EffectKind::Reverb, 0));
    let first = rig.project.mixer.tracks[1].id;
    rig.track_mut(first).sends.push(Send {
        target: keys,
        gain: 0.3,
    });
    let synth = rig.synth_on(plain_synth(), keys);
    let second = rig.project.patterns[1].id;
    rig.note_in(second, synth, 0, 1_500).key = 52;
    rig.note(synth, 240, 2_000).key = 57;
    automate_everything(&mut rig, synth);
    (rig, synth)
}

#[test]
fn automation_never_allocates_or_frees_on_the_audio_path() {
    let (mut rig, synth) = automated_rig();
    let curves: Vec<AutomationId> = rig.project.automations.iter().map(|a| a.id).collect();
    let (mut processor, controller) = rig.processor(48_000);
    controller.set_transport(TransportPatch {
        mode: Some(PlayMode::Song),
        loop_song: Some(true),
        ..TransportPatch::default()
    });

    let mut out = vec![0.0_f32; 4_096 * 2];
    let sizes = [128, 1, 7, 64, 480, 1_024, 4_096, 33, 63, 65];
    let mut calls = 0;
    let mut loudest = 0.0_f32;
    let mut reported = 0;
    controller.play();
    for step in 0..600_u32 {
        match step % 100 {
            13 => controller.seek(f64::from(step) * 11.0),
            31 => controller.note_on(synth, 64, 0.8),
            37 => controller.note_off(synth, 64),
            55 => controller.stop(),
            57 => controller.play(),
            70 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Pattern),
                ..TransportPatch::default()
            }),
            72 => controller.play(),
            85 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Song),
                ..TransportPatch::default()
            }),
            87 => controller.play(),
            _ => {}
        }
        if step % 4 == 0 {
            // Every curve is redrawn, the tempo's among them, a clip is
            // moved, muted and brought back, and a fader is moved under
            // its automation.
            for (index, id) in curves.iter().enumerate() {
                let points = &mut rig.automation_mut(*id).points;
                let moved = (step as usize + index) % points.len();
                points[moved].value = 0.15 + ((step + index as u32) % 17) as f32 * 0.05;
                points[moved].curve = ((step % 9) as f32 - 4.0) * 0.2;
            }
            let clips = rig.project.playlist.clips.len();
            let clip = &mut rig.project.playlist.clips[(step as usize / 4) % clips];
            if matches!(clip.content, ClipContent::Automation { .. }) {
                clip.muted = step % 24 == 0;
                clip.offset = step % 200;
            }
            rig.project.mixer.tracks[1].volume = 0.6 + (step % 5) as f32 * 0.1;
            controller.set_project(&rig.project, &rig.pool);
        }
        if step == 450 {
            // The track the effects are on goes, with its automations
            // still on the playlist of a project that is no longer whole.
            rig.project.mixer.tracks.remove(3);
            controller.set_project(&rig.project, &rig.pool);
        }

        let frames = sizes[step as usize % sizes.len()];
        let block = &mut out[..frames * 2];
        calls += allocator_calls(|| processor.process(block));
        loudest = loudest.max(peak(block));
        // Reading the frame is the control side's business.
        reported = reported.max(controller.frame().automated.len());
    }

    assert_eq!(calls, 0, "the audio path used the allocator");
    assert!(loudest > 0.05, "the test played nothing");
    assert!(
        reported >= 8,
        "only {reported} automations were ever at work"
    );
}

#[test]
fn holding_and_letting_go_of_automation_never_allocates_or_frees_on_the_audio_path() {
    // The song does not loop, so it ends by itself, with reverb and echoes
    // still ringing under its automation.
    let (mut rig, synth) = automated_rig();
    let (mut processor, controller) = rig.processor(48_000);
    controller.set_transport(TransportPatch {
        mode: Some(PlayMode::Song),
        loop_song: Some(false),
        ..TransportPatch::default()
    });
    let preview = sine(22_050, 880.0, 0.1);

    let mut out = vec![0.0_f32; 4_096 * 2];
    let sizes = [128, 1, 7, 64, 480, 1_024, 4_096, 33, 63, 65];
    let mut calls = 0;
    let mut loudest = 0.0_f32;
    // Looks at the engine that found it stopped with its automation still
    // at work, and that found it let go again.
    let (mut held, mut let_go) = (0, 0);
    // Which time the song is being played, the steps since it started and
    // the steps since it stopped.
    let (mut round, mut played, mut stopped) = (0_u32, 0_u32, None::<u32>);
    controller.play();
    for step in 0..3_000_u32 {
        let frame = controller.frame();
        if stopped.is_none() && !frame.playing {
            stopped = Some(0);
        }
        let edit = |rig: &mut Rig| {
            rig.project.mixer.tracks[1].volume = 0.6 + (step % 5) as f32 * 0.1;
            let curves = rig.project.automations.len();
            let points = &mut rig.project.automations[step as usize % curves].points;
            points[0].value = 0.2 + (step % 7) as f32 * 0.1;
            controller.set_project(&rig.project, &rig.pool);
        };
        let mut again = false;
        match (round % 3, stopped) {
            // Left to end by itself: an edit while it rings out, then a
            // note by hand, which lets go.
            (0, Some(6)) => edit(&mut rig),
            (0, Some(12)) => controller.note_on(synth, 64, 0.8),
            (0, Some(16)) => controller.note_off(synth, 64),
            (0, Some(30)) => again = true,
            // Stopped part way, edited, stopped again while it rings,
            // moved and played.
            (1, None) if played == 40 => controller.stop(),
            (1, Some(5)) => edit(&mut rig),
            (1, Some(10)) => controller.stop(),
            (1, Some(14)) => controller.seek(f64::from(step % 900)),
            (1, Some(20)) => again = true,
            // Stopped and left until everything has rung out, then a
            // preview.
            (2, None) if played == 60 => controller.stop(),
            (2, Some(waited)) if waited > 3 && frame.automated.is_empty() => {
                controller.preview(preview.clone());
                again = true;
            }
            _ => {}
        }
        if let Some(waited) = &mut stopped {
            *waited += 1;
            if frame.automated.is_empty() {
                let_go += 1;
            } else {
                held += 1;
            }
        }
        played += 1;
        if again {
            controller.play();
            (round, played, stopped) = (round + 1, 0, None);
        }

        let frames = sizes[step as usize % sizes.len()];
        let block = &mut out[..frames * 2];
        calls += allocator_calls(|| processor.process(block));
        loudest = loudest.max(peak(block));
    }

    assert_eq!(calls, 0, "the audio path used the allocator");
    assert!(loudest > 0.05, "the test played nothing");
    assert!(round >= 6, "the song was only played {round} times");
    assert!(held > 60, "automation was held for only {held} looks");
    assert!(let_go > 60, "automation was let go for only {let_go} looks");
}

#[test]
fn processing_never_allocates_or_frees() {
    let mut rig = busy_rig();
    let hat = rig.project.channels[1].id;
    let pad = rig.project.channels[3].id;
    let audio_clips = rig.project.playlist.clips.iter();
    let audio_clips: Vec<ClipId> = audio_clips
        .filter(|clip| matches!(clip.content, ClipContent::Audio { .. }))
        .map(|clip| clip.id)
        .collect();
    let (riser, reversed) = (audio_clips[0], audio_clips[1]);
    let (mut processor, controller) = rig.processor(48_000);
    let preview = sine(22_050, 880.0, 0.5);

    let mut out = vec![0.0_f32; 4_096 * 2];
    let sizes = [128, 1, 7, 64, 480, 1_024, 4_096, 33];
    let mut calls = 0;
    let mut loudest = 0.0_f32;
    controller.play();

    for step in 0..600_u32 {
        // Everything the control side does between buffers may allocate.
        // What the audio thread does with it may not.
        match step % 120 {
            5 => controller.note_on(pad, 64, 0.9),
            9 => controller.note_on(hat, 60, 0.7),
            15 => controller.note_off(pad, 64),
            20 => controller.preview(preview.clone()),
            24 => controller.preview(level(48_000, 0.1, 0.05)),
            30 => controller.stop_preview(),
            35 => controller.seek(f64::from(step) * 7.5),
            40 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Song),
                loop_song: Some(step % 240 < 120),
                ..TransportPatch::default()
            }),
            44 => controller.play(),
            60 => controller.stop(),
            62 => controller.play(),
            80 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Pattern),
                pattern: Some(rig.project.patterns[(step as usize / 120) % 2].id),
                ..TransportPatch::default()
            }),
            82 => controller.play(),
            100 => controller.set_output_gain(if step % 240 < 120 { 0.5 } else { 1.0 }),
            _ => {}
        }
        if step % 3 == 0 {
            // An edit and a new plan: a fader, the tempo, a toggled step,
            // and the audio clips: a level, a fade, a mute, and now and
            // then a new place or pitch, which starts a clip over.
            rig.project.channels[0].volume = 0.5 + (step % 7) as f32 * 0.05;
            rig.project.mixer.tracks[1].pan = (step % 5) as f32 * 0.2 - 0.4;
            rig.project.settings.tempo_bpm = 120.0 + f64::from(step % 50);
            let muted = &mut rig.project.channels[2].muted;
            *muted = !*muted;
            if step <= 330 {
                rig.audio_mut(riser, |audio| {
                    audio.gain = 0.4 + (step % 5) as f32 * 0.1;
                    audio.fade_out = 300 + step % 11 * 40;
                    audio.pitch = if step % 21 == 0 { -3.0 } else { 2.0 };
                });
                rig.clip_mut(riser).start = if step % 12 == 0 { 300 } else { 500 };
            }
            rig.clip_mut(reversed).muted = step % 9 == 0;
            controller.set_project(&rig.project, &rig.pool);
        }
        if step == 330 {
            // An audio clip goes with its sample while it may be sounding,
            // so a slot ends up as the last owner of the audio.
            let windfall_project::ClipContent::Audio { sample, .. } = rig.clip_mut(riser).content
            else {
                panic!("not an audio clip");
            };
            rig.project.playlist.clips.retain(|clip| clip.id != riser);
            rig.project.samples.retain(|held| held.id != sample);
            rig.pool.remove(sample);
            controller.set_project(&rig.project, &rig.pool);
        }
        if step == 250 {
            // The sample of a sounding channel is swapped for another, and
            // the pool lets go of the old one, so a voice ends up as its
            // last owner.
            let replacement = rig.sample(sine(48_000, 330.0, 1.0));
            let old = rig.sampler_mut(pad).sample.replace(replacement);
            rig.pool.remove(old.expect("the pad had a sample"));
            controller.set_project(&rig.project, &rig.pool);
        }
        if step == 400 {
            // A channel and a mixer track disappear while they sound.
            rig.project.channels.retain(|channel| channel.id != hat);
            rig.project.mixer.tracks.remove(2);
            controller.set_project(&rig.project, &rig.pool);
        }

        let frames = sizes[step as usize % sizes.len()];
        let block = &mut out[..frames * 2];
        calls += allocator_calls(|| processor.process(block));
        loudest = loudest.max(peak(block));
    }

    assert_eq!(calls, 0, "the audio path used the allocator");
    assert!(loudest > 0.1, "the test played nothing");
    assert!(controller.frame().voices <= 320);
}

/// One effect of each kind at settings that depend on `step`, so that a
/// project built from them changes every setting from plan to plan.
fn effect_at(kind: EffectKind, step: u32) -> EffectParams {
    let turn = (step % 9) as f32 / 8.0;
    match kind {
        EffectKind::Eq => {
            let mut eq = EqParams::default();
            eq.peak2.gain_db = turn * 12.0 - 6.0;
            eq.peak2.frequency_hz = 300.0 + turn * 3_000.0;
            EffectParams::Eq(eq)
        }
        EffectKind::Compressor => EffectParams::Compressor(CompressorParams {
            threshold_db: -30.0 + turn * 20.0,
            ratio: 2.0 + turn * 6.0,
            ..CompressorParams::default()
        }),
        EffectKind::Limiter => EffectParams::Limiter(LimiterParams {
            ceiling_db: -6.0 * turn,
            lookahead_ms: 1.0 + turn * 19.0,
            ..LimiterParams::default()
        }),
        EffectKind::Reverb => EffectParams::Reverb(ReverbParams {
            size: turn,
            decay_s: 0.3 + turn * 2.0,
            mix: 0.2 + turn * 0.5,
            ..ReverbParams::default()
        }),
        EffectKind::Delay => EffectParams::Delay(DelayParams {
            sync: step.is_multiple_of(2),
            time_ms: 50.0 + turn * 400.0,
            feedback: turn * 0.6,
            ..DelayParams::default()
        }),
        EffectKind::Balance
        | EffectKind::DcBlock
        | EffectKind::ChannelMute
        | EffectKind::Polarity
        | EffectKind::StereoMatrix
        | EffectKind::SoftClipper
        | EffectKind::Distortion
        | EffectKind::FastLowpass
        | EffectKind::SelectableFilter
        | EffectKind::BassShelf
        | EffectKind::Lofi
        | EffectKind::Chorus
        | EffectKind::Flanger
        | EffectKind::Phaser
        | EffectKind::BandSplit
        | EffectKind::MultibandCompressor
        | EffectKind::MultibandMaximizer
        | EffectKind::TransientShaper
        | EffectKind::TransientSplit
        | EffectKind::OneKnob
        | EffectKind::BassHarmonics
        | EffectKind::Exciter
        | EffectKind::Waveshaper
        | EffectKind::Overdrive
        | EffectKind::GuitarRack
        | EffectKind::DriveChain
        | EffectKind::VintageChorus
        | EffectKind::HyperChorus
        | EffectKind::VintagePhaser
        | EffectKind::StackedFlanger
        | EffectKind::BandDelay
        | EffectKind::Room
        | EffectKind::Spreader
        | EffectKind::StereoEnhancer
        | EffectKind::VolumeGate
        | EffectKind::TimeTransport
        | EffectKind::Scratch
        | EffectKind::PerformanceRack
        | EffectKind::Convolver
        | EffectKind::FrequencyShifter
        | EffectKind::PitchShift
        | EffectKind::PitchCorrect
        | EffectKind::Vocoder
        | EffectKind::EchoBank
        | EffectKind::FrequencyDelay
        | EffectKind::SevenBand
        | EffectKind::MorphEq
        | EffectKind::FilterBank
        | EffectKind::XyPad
        | EffectKind::XyzPad
        | EffectKind::PanLfo
        | EffectKind::EnvelopeFollower
        | EffectKind::NoteEnvelope
        | EffectKind::LushSpace
        | EffectKind::Tuner
        | EffectKind::StageStack
        | EffectKind::ControlSurface
        | EffectKind::SendTap
        | EffectKind::MasterLimiter
        | EffectKind::FormulaSource
        | EffectKind::KeyboardSource
        | EffectKind::VoltageSource => {
            // Sweep each declared scalar across its range, including new
            // processors, while keeping this exhaustive for future variants.
            let mut params = kind.default_params();
            for (index, info) in kind.descriptors().iter().enumerate() {
                params.set(index, info.min + turn * (info.max - info.min));
            }
            params
        }
    }
}

#[test]
fn effects_and_instruments_never_allocate_or_free_on_the_audio_path() {
    let mut rig = busy_rig();
    let drums = rig.project.mixer.tracks[1].id;
    let space = rig.project.mixer.tracks[2].id;
    let keys = rig.track();
    rig.track_mut(keys).sends.push(Send {
        target: space,
        gain: 0.5,
    });
    let mut pad = plain_synth();
    pad.unison_voices = 4;
    pad.amp_envelope.release_ms = 120.0;
    let synth = rig.synth_on(pad, keys);
    for (index, key) in [45, 57, 60, 64, 67].into_iter().enumerate() {
        rig.note(synth, index as u32 * 700, 900).key = key;
    }
    let second = rig.project.patterns[1].id;
    rig.note_in(second, synth, 0, 1_500).key = 52;

    // Every kind of effect, spread over tracks to respect the slot limit; a limiter that looks
    // ahead on another and on the master, so latency is compensated on
    // several paths at once.
    // Keep one free slot on keys for the cross-track move below. Every extra
    // rack gets its own sounding sampler so new effects process real input.
    let slots = windfall_project::MAX_EFFECT_SLOTS;
    let extra_count = EffectKind::ALL
        .len()
        .saturating_sub(slots * 2 - 1)
        .div_ceil(slots);
    let extra_racks: Vec<TrackId> = (0..extra_count)
        .map(|_| {
            let track = rig.track();
            let channel = rig.channel_on(level(48_000, 0.1, 2.0), track);
            rig.steps(channel, &[0]);
            track
        })
        .collect();
    let mut chain: Vec<EffectId> = EffectKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| {
            rig.effect(
                if index < slots {
                    space
                } else if index < slots * 2 - 1 {
                    keys
                } else {
                    extra_racks[(index - (slots * 2 - 1)) / slots]
                },
                effect_at(kind, 0),
            )
        })
        .collect();
    rig.effect(drums, limiter(-3.0));
    let master = rig.effect(TrackId::MASTER, idle_limiter(5.0));
    let (mut processor, controller) = rig.processor(48_000);

    let mut out = vec![0.0_f32; 4_096 * 2];
    let sizes = [128, 1, 7, 64, 480, 1_024, 4_096, 33];
    let mut calls = 0;
    let mut loudest = 0.0_f32;
    let mut reduced = 0.0_f32;
    let mut extra: Option<windfall_project::ChannelId> = None;
    controller.play();

    for step in 0..500_u32 {
        match step % 100 {
            3 => controller.note_on(synth, 72, 0.8),
            11 => controller.note_off(synth, 72),
            20 => controller.seek(f64::from(step) * 5.0),
            35 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Song),
                loop_song: Some(true),
                ..TransportPatch::default()
            }),
            37 => controller.play(),
            55 => controller.stop(),
            57 => controller.play(),
            70 => controller.set_transport(TransportPatch {
                mode: Some(PlayMode::Pattern),
                pattern: Some(rig.project.patterns[(step as usize / 100) % 2].id),
                ..TransportPatch::default()
            }),
            72 => controller.play(),
            _ => {}
        }
        let mut edited = step % 2 == 0;
        if edited {
            // Every setting of every effect, a fader, the synth and the
            // tempo, in one plan.
            for (id, kind) in chain.clone().into_iter().zip(EffectKind::ALL) {
                if rig
                    .project
                    .mixer
                    .tracks
                    .iter()
                    .any(|t| t.effect(id).is_some())
                {
                    rig.effect_mut(id).params = effect_at(kind, step);
                }
            }
            rig.effect_mut(master).params = idle_limiter(1.0 + (step % 7) as f32 * 3.0);
            rig.effect_mut(master).enabled = step % 12 < 8;
            rig.effect_mut(master).mix = 0.5 + (step % 3) as f32 * 0.25;
            rig.synth_mut(synth).filter.cutoff_hz = 500.0 + (step % 11) as f32 * 800.0;
            rig.synth_mut(synth).gain = 0.3 + (step % 4) as f32 * 0.1;
            rig.project.channels[0].volume = 0.5 + (step % 7) as f32 * 0.05;
            rig.project.settings.tempo_bpm = 120.0 + f64::from(step % 50);
        }
        // Until the track that holds the chain is itself removed.
        let chained = if chain.is_empty() { 99 } else { step % 50 };
        match chained {
            // An effect goes, and comes back as a new one two plans later.
            7 => {
                rig.remove_effect(chain[3]);
                edited = true;
            }
            9 => {
                chain[3] = rig.effect(space, effect_at(EffectKind::Reverb, step));
                edited = true;
            }
            // One moves to the front of its chain, and one to another track
            // and back.
            15 => {
                let slot = rig.remove_effect(chain[4]);
                rig.track_mut(space).effects.insert(0, slot);
                edited = true;
            }
            21 => {
                let slot = rig.remove_effect(chain[1]);
                rig.track_mut(keys).effects.push(slot);
                edited = true;
            }
            27 => {
                let slot = rig.remove_effect(chain[1]);
                rig.track_mut(space).effects.push(slot);
                edited = true;
            }
            _ => {}
        }
        match step % 50 {
            // A second synth comes while the first is sounding, plays a
            // note by hand, and goes again with the note still down.
            31 => {
                let id = rig.synth_on(plain_synth(), keys);
                extra = Some(id);
                edited = true;
            }
            33 => {
                if let Some(id) = extra {
                    controller.note_on(id, 55, 1.0);
                }
            }
            39 => {
                if let Some(id) = extra.take() {
                    rig.project.channels.retain(|channel| channel.id != id);
                    edited = true;
                }
            }
            // The first synth moves to another track and back.
            43 => {
                rig.channel_mut(synth).mixer_track = drums;
                edited = true;
            }
            45 => {
                rig.channel_mut(synth).mixer_track = keys;
                edited = true;
            }
            _ => {}
        }
        if edited {
            controller.set_project(&rig.project, &rig.pool);
        }
        if step == 300 {
            // A track goes with its effects and with sound still in them.
            rig.project.mixer.tracks.retain(|track| track.id != space);
            chain.clear();
            controller.set_project(&rig.project, &rig.pool);
        }

        let frames = sizes[step as usize % sizes.len()];
        let block = &mut out[..frames * 2];
        calls += allocator_calls(|| processor.process(block));
        loudest = loudest.max(peak(block));
        // Reading the frame is the control side's business, and frees what
        // the audio thread handed back.
        let frame = controller.frame();
        for reading in &frame.gain_reductions {
            reduced = reduced.max(reading.db);
        }
    }

    assert_eq!(calls, 0, "the audio path used the allocator");
    assert!(loudest > 0.1, "the test played nothing");
    assert!(reduced > 1.0, "no limiter or compressor ever worked");
}

#[test]
fn a_voice_that_outlives_every_other_owner_of_its_sample_frees_nothing() {
    let mut rig = Rig::new();
    let channel = rig.channel(level(48_000, 0.5, 0.05));
    rig.steps(channel, &[0]);
    let (mut processor, controller) = rig.processor(48_000);
    controller.play();
    // 64 frames a call, so the 4 ms fade below spans several calls.
    let mut out = vec![0.0_f32; 128];
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    assert_eq!(controller.frame().voices, 1);

    // The project moves on without the sample, and every handle to it on
    // this side is dropped.
    let empty = Rig::new();
    controller.set_project(&empty.project, &SamplePool::new());
    drop(rig);
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    // This call drops the plan the audio thread just handed back. Now only
    // the fading voice holds the audio.
    assert_eq!(controller.frame().voices, 1);

    let mut calls = 0;
    for _ in 0..8 {
        calls += allocator_calls(|| processor.process(&mut out));
    }
    assert_eq!(
        calls, 0,
        "ending the voice freed its sample on the audio path"
    );
    // The audio went back to this side instead, and this call frees it.
    assert_eq!(controller.frame().voices, 0);
}

#[test]
fn spectral_clip_playback_loop_seek_and_replacement_never_touch_allocator() {
    let mut rig = Rig::new();
    let lane = rig.playlist_track();
    let id = rig.audio_clip(lane, sine(48_000, 440.0, 1.0), TrackId::MASTER, 0, 1920);
    if let ClipContent::Audio { stretch, pitch, .. } = &mut rig
        .project
        .playlist
        .clips
        .iter_mut()
        .find(|c| c.id == id)
        .unwrap()
        .content
    {
        *stretch = windfall_project::ClipStretch::Spectral {
            ratio: 1.5,
            quality: windfall_project::ClipStretchQuality::Standard,
            formants: false,
        };
        *pitch = 12.0;
    }
    let (mut processor, controller) = rig.song_processor(48_000);
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..Default::default()
    });
    controller.play();
    let mut output = [0.0; 960];
    assert_eq!(
        allocator_calls(|| {
            for _ in 0..300 {
                processor.process(&mut output);
            }
        }),
        0
    );
    controller.seek(500.0);
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
}
