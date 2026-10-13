//! Utility effects through real routing, automation, edits and rendering.
use crate::support::{Rig, idle_limiter, impulse, left, level, peak, right, rms, run, sine};
use windfall_dsp::{DcBlockParams, DistortionParams, EffectKind, EffectParams, StereoMatrixParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{AutomationTarget, TrackId};

const RATE: u32 = 48_000;
const KINDS: [EffectKind; 7] = [
    EffectKind::Balance,
    EffectKind::DcBlock,
    EffectKind::ChannelMute,
    EffectKind::Polarity,
    EffectKind::StereoMatrix,
    EffectKind::SoftClipper,
    EffectKind::Distortion,
];

fn matrix(left_delay_ms: f32, right_delay_ms: f32) -> EffectParams {
    EffectParams::StereoMatrix(StereoMatrixParams {
        left_delay_ms,
        right_delay_ms,
        ..Default::default()
    })
}
fn configured(kind: EffectKind) -> EffectParams {
    if kind == EffectKind::StereoMatrix {
        matrix(2.0, 2.0)
    } else {
        kind.default_params()
    }
}

fn guarded_run(
    processor: &mut windfall_engine::Processor,
    frames: usize,
    block: usize,
) -> Vec<f32> {
    let mut audio = vec![0.0; frames * 2];
    for out in audio.chunks_mut(block * 2) {
        assert_eq!(
            crate::realtime::allocator_calls(|| processor.process(out)),
            0
        );
    }
    audio
}

/// The controller query never destroys native owners under a caller guard.
/// Tests explicitly collect after their control/document work is complete.
fn observe_and_retire(controller: &windfall_engine::Controller) {
    controller.frame();
    let mut retirement = windfall_engine::ProjectRetirement::default();
    controller.take_retired(&mut retirement);
}

#[test]
fn utility_effects_r3_departing_settled_limiter_keeps_its_compensation_splice() {
    let mut rig = Rig::new();
    let (first, second) = (rig.track(), rig.track());
    let tone = sine(RATE, 500.0, 0.25);
    let up = rig.channel_on(tone.clone(), first);
    let down = rig.channel_on(crate::support::inverted(&tone), second);
    for channel in [up, down] {
        rig.channel_mut(channel).volume = 0.5;
        rig.steps(channel, &[0]);
    }
    rig.effect(first, matrix(1.0, 1.0));
    let limiter = rig.effect(first, idle_limiter(1.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    assert!(peak(&run(&mut processor, 4092, 137)) < 1e-6);
    rig.remove_effect(limiter);
    controller.set_project(&rig.project, &rig.pool);
    let audio = run(&mut processor, 1000, 29);
    assert!(
        peak(&audio) < 1e-6,
        "departing limiter residual {}, first {}",
        peak(&audio),
        audio[0]
    );
    assert_eq!(controller.latency_frames(), 48);
}

#[derive(Debug, Default)]
struct R3PluginStats {
    creates: std::sync::atomic::AtomicUsize,
    processes: std::sync::atomic::AtomicUsize,
    drops: std::sync::atomic::AtomicUsize,
}
#[derive(Debug, Default)]
struct R3DelayFactory(std::sync::Arc<R3PluginStats>, std::sync::atomic::AtomicU64);

impl windfall_engine::plugins::PluginFactory for R3DelayFactory {
    fn revision(&self) -> u64 {
        self.1.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn effect(
        &self,
        _: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedEffect>, String> {
        self.0
            .creates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Box::new(R3HostedDelay {
            ring: [[0.0; 2]; 32],
            write: 0,
            stats: self.0.clone(),
        }))
    }
    fn instrument(
        &self,
        _: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedInstrument>, String> {
        Err("effect fixture".into())
    }
}

struct R3HostedDelay {
    ring: [[f32; 2]; 32],
    write: usize,
    stats: std::sync::Arc<R3PluginStats>,
}
impl Drop for R3HostedDelay {
    fn drop(&mut self) {
        self.stats
            .drops
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
impl windfall_engine::plugins::HostedEffect for R3HostedDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.stats
            .processes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (left, right) in left.iter_mut().zip(right) {
            let old = self.ring[self.write];
            self.ring[self.write] = [*left, *right];
            self.write = (self.write + 1) % 32;
            [*left, *right] = old;
        }
    }
    fn set_param(&mut self, _: u32, _: f32) {}
    fn set_tempo(&mut self, _: f32) {}
    fn latency(&self) -> usize {
        32
    }
    fn tail(&self) -> usize {
        32
    }
}

#[test]
fn utility_effects_r3_fixed_stage_removal_before_and_after_matrices_is_allocation_free() {
    for hosted in [false, true] {
        for place in 0..=2 {
            for enabled in [true, false] {
                let mut rig = Rig::new();
                let (first, second) = (rig.track(), rig.track());
                let unused = rig.track();
                let tone = sine(RATE, 500.0, 1.0);
                let up = rig.channel_on(tone.clone(), first);
                let down = rig.channel_on(crate::support::inverted(&tone), second);
                for channel in [up, down] {
                    rig.channel_mut(channel).volume = 0.5;
                    rig.steps(channel, &[0]);
                }
                rig.effect(first, matrix(1.0, 1.0));
                rig.effect(first, matrix(1.0, 1.0));
                let fixed = rig.effect(
                    first,
                    if hosted {
                        EffectKind::Balance.default_params()
                    } else {
                        idle_limiter(1.0)
                    },
                );
                rig.effect_mut(fixed).enabled = enabled;
                let slot = rig.remove_effect(fixed);
                rig.track_mut(first).effects.insert(place, slot);
                let factory = std::sync::Arc::new(R3DelayFactory::default());
                if hosted {
                    rig.project.plugins.push(windfall_project::PluginBinding {
                        sidechain_input: None,
                        auxiliary_inputs: Vec::new(),
                        capabilities: Default::default(),
                        wrapper: Default::default(),
                        target: windfall_project::PluginTarget::Effect { effect: fixed },
                        format: "clap".into(),
                        path: "r3-prepared-delay.clap".into(),
                        id: "r3-delay".into(),
                        name: "Prepared delay".into(),
                        state: vec![],
                        parameters: vec![],
                    });
                    rig.pool.set_plugin_factory(factory.clone());
                }
                let (mut processor, controller) = rig.processor(RATE);
                controller.play();
                assert!(peak(&run(&mut processor, 4092, 137)) < 1e-6);
                rig.remove_effect(fixed);
                // Remove the binding too: the retiring stage's native latency
                // must come from the held ledger, not the new project.
                rig.project.plugins.retain(|binding| {
                    binding.target != windfall_project::PluginTarget::Effect { effect: fixed }
                });
                controller.set_project(&rig.project, &rig.pool);
                assert_eq!(controller.latency_frames(), 96);
                let mut out = [0.0; 137 * 2];
                let mut residual = 0.0_f32;
                let mut remaining = 80;
                for block in [1, 7, 137].into_iter().cycle() {
                    if remaining == 0 {
                        break;
                    }
                    let count = remaining.min(block);
                    let out = &mut out[..count * 2];
                    assert_eq!(
                        crate::realtime::allocator_calls(|| processor.process(out)),
                        0
                    );
                    residual = residual.max(peak(out));
                    remaining -= count;
                }
                // An intervening plan must neither restart the outer splice
                // nor forget the removed plugin's latency while it is audible.
                for edit in 0..40 {
                    rig.track_mut(unused).volume = if edit % 2 == 0 { 0.75 } else { 0.5 };
                    controller.set_project(&rig.project, &rig.pool);
                    assert_eq!(
                        crate::realtime::allocator_calls(|| processor.process(&mut out[..2])),
                        0
                    );
                    residual = residual.max(peak(&out[..2]));
                    observe_and_retire(&controller);
                }
                for _ in 0..8 {
                    assert_eq!(
                        crate::realtime::allocator_calls(|| processor.process(&mut out)),
                        0
                    );
                    residual = residual.max(peak(&out));
                }
                assert!(
                    residual < 1e-6,
                    "fixed removal residual {residual}, hosted {hosted}, place {place}, enabled {enabled}"
                );
                observe_and_retire(&controller);
                controller.set_project(&rig.project, &rig.pool);
                assert_eq!(
                    crate::realtime::allocator_calls(|| processor.process(&mut out)),
                    0
                );
                assert!(peak(&out) < 1e-6);
                observe_and_retire(&controller);
                if hosted {
                    assert_eq!(
                        factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
                        1,
                        "the completed native departure must retire on the control side"
                    );
                    let processes = factory
                        .0
                        .processes
                        .load(std::sync::atomic::Ordering::Relaxed);
                    controller.set_project(&rig.project, &rig.pool);
                    assert_eq!(
                        crate::realtime::allocator_calls(|| processor.process(&mut out)),
                        0
                    );
                    observe_and_retire(&controller);
                    assert_eq!(
                        factory
                            .0
                            .processes
                            .load(std::sync::atomic::Ordering::Relaxed),
                        processes
                    );
                }
            }
        }
    }
}

#[test]
fn utility_effects_r3_an_intervening_plan_preserves_the_departing_splices_actual_duration() {
    let mut rig = Rig::new();
    let (first, unused) = (rig.track(), rig.track());
    let channel = rig.channel_on(sine(RATE, 500.0, 1.0), first);
    rig.channel_mut(channel).volume = 0.5;
    rig.steps(channel, &[0]);
    rig.effect(first, matrix(1.0, 1.0));
    let fixed = rig.effect(first, idle_limiter(1.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    rig.remove_effect(fixed);
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 80, 7);
    for edit in 0..40 {
        rig.track_mut(unused).volume = if edit % 2 == 0 { 0.75 } else { 0.5 };
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(guarded_run(&mut processor, 1, 1));
        observe_and_retire(&controller);
    }
    audio.extend(guarded_run(&mut processor, 1000, 29));
    let step = left(&audio)
        .windows(2)
        .map(|p| (p[1] - p[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        step < 0.04,
        "intervening plan cut the departing splice: step {step}"
    );
}

#[test]
fn utility_effects_r3_unheard_speculative_native_slots_retire_without_activation() {
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(level(RATE, 1.0, 1.0), first);
    rig.steps(channel, &[0]);
    let factory = std::sync::Arc::new(R3DelayFactory::default());
    rig.pool.set_plugin_factory(factory.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    for _ in 0..40 {
        let plugin = rig.effect(first, EffectKind::Balance.default_params());
        rig.project.plugins.push(windfall_project::PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            capabilities: Default::default(),
            wrapper: Default::default(),
            target: windfall_project::PluginTarget::Effect { effect: plugin },
            format: "clap".into(),
            path: "r3-speculative-delay.clap".into(),
            id: "r3-delay".into(),
            name: "Prepared delay".into(),
            state: vec![],
            parameters: vec![],
        });
        controller.set_project(&rig.project, &rig.pool);
        rig.remove_effect(plugin);
        rig.project.plugins.clear();
        // Both plans are queued before a callback. The prepared native owner
        // has never contributed to audio and must not become a departing one.
        controller.set_project(&rig.project, &rig.pool);
        assert!(guarded_run(&mut processor, 7, 7).iter().all(|s| *s == 1.0));
        observe_and_retire(&controller);
        assert_eq!(controller.latency_frames(), 0);
    }
    assert_eq!(
        factory
            .0
            .processes
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert_eq!(
        factory.0.creates.load(std::sync::atomic::Ordering::Relaxed),
        40
    );
    assert_eq!(
        factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
        40
    );
}

#[test]
fn utility_effects_r3_restored_native_id_uses_a_new_active_owner_not_a_retiring_one() {
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(level(RATE, 1.0, 1.0), first);
    rig.steps(channel, &[0]);
    let plugin = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = windfall_project::PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
        target: windfall_project::PluginTarget::Effect { effect: plugin },
        format: "clap".into(),
        path: "r3-restored-delay.clap".into(),
        id: "r3-delay".into(),
        name: "Prepared delay".into(),
        state: vec![],
        parameters: vec![],
    };
    rig.project.plugins.push(binding.clone());
    let factory = std::sync::Arc::new(R3DelayFactory::default());
    rig.pool.set_plugin_factory(factory.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(plugin);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert!(guarded_run(&mut processor, 80, 7).iter().all(|s| *s == 1.0));
    rig.track_mut(first).effects.insert(0, slot);
    rig.project.plugins.push(binding);
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 1000, 29)
            .iter()
            .all(|s| *s == 1.0)
    );
    observe_and_retire(&controller);
    // A completed departure is omitted by the next control snapshot; its
    // native owner is destroyed when that old state is collected off RT.
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 137, 137);
    observe_and_retire(&controller);
    assert_eq!(
        factory.0.creates.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert_eq!(
        factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(controller.latency_frames(), 80);
}

fn r4_restore_tone(hosted: bool) {
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(sine(RATE, 500.0, 1.0), first);
    rig.channel_mut(channel).volume = 0.5;
    rig.steps(channel, &[0]);
    let fixed = rig.effect(
        first,
        if hosted {
            EffectKind::Balance.default_params()
        } else {
            idle_limiter(1.0)
        },
    );
    rig.effect(first, matrix(1.0, 1.0));
    let binding = windfall_project::PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
        target: windfall_project::PluginTarget::Effect { effect: fixed },
        format: "clap".into(),
        path: "r4-restored-delay.clap".into(),
        id: "r4-delay".into(),
        name: "Prepared delay".into(),
        state: vec![],
        parameters: vec![],
    };
    let factory = std::sync::Arc::new(R3DelayFactory::default());
    if hosted {
        rig.project.plugins.push(binding.clone());
        rig.pool.set_plugin_factory(factory.clone());
    }
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(fixed);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 80, 7);
    rig.track_mut(first).effects.insert(0, slot);
    if hosted {
        rig.project.plugins.push(binding);
    }
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 1000, 29));
    let (at, step) = left(&audio)
        .windows(2)
        .enumerate()
        .map(|(at, p)| (at + 1, (p[1] - p[0]).abs()))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    println!("restored tone maximum step {step} at {at}, hosted {hosted}");
    assert!(
        step < 0.04,
        "restored tone step {step} at {at}, hosted {hosted}"
    );
    if hosted {
        observe_and_retire(&controller);
        controller.set_project(&rig.project, &rig.pool);
        guarded_run(&mut processor, 137, 137);
        observe_and_retire(&controller);
        assert_eq!(
            factory.0.creates.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        assert_eq!(
            factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }
}

#[test]
fn utility_effects_r4_limiter_restore_retains_the_unfinished_removal_tone() {
    r4_restore_tone(false);
}

#[test]
fn utility_effects_r4_hosted_restore_retains_the_unfinished_removal_tone() {
    r4_restore_tone(true);
}

#[test]
fn utility_effects_r4_repeated_restore_remove_tones_before_between_after_matrices() {
    for hosted in [false, true] {
        for place in 0..=2 {
            let mut rig = Rig::new();
            let first = rig.track();
            let channel = rig.channel_on(sine(RATE, 500.0, 1.0), first);
            rig.channel_mut(channel).volume = 0.5;
            rig.steps(channel, &[0]);
            rig.effect(first, matrix(1.0, 1.0));
            rig.effect(first, matrix(1.0, 1.0));
            let fixed = rig.effect(
                first,
                if hosted {
                    EffectKind::Balance.default_params()
                } else {
                    idle_limiter(1.0)
                },
            );
            let mut slot = Some(rig.remove_effect(fixed));
            rig.track_mut(first)
                .effects
                .insert(place, slot.take().unwrap());
            let binding = windfall_project::PluginBinding {
                sidechain_input: None,
                auxiliary_inputs: Vec::new(),
                capabilities: Default::default(),
                wrapper: Default::default(),
                target: windfall_project::PluginTarget::Effect { effect: fixed },
                format: "clap".into(),
                path: "r4-rapid-restored-delay.clap".into(),
                id: "r4-delay".into(),
                name: "Prepared delay".into(),
                state: vec![],
                parameters: vec![],
            };
            let factory = std::sync::Arc::new(R3DelayFactory::default());
            if hosted {
                rig.project.plugins.push(binding.clone());
                rig.pool.set_plugin_factory(factory.clone());
            }
            let (mut processor, controller) = rig.processor(RATE);
            controller.play();
            let mut audio = guarded_run(&mut processor, 4092, 137);
            for (restore, frames, block) in [
                (false, 80, 7),
                (true, 400, 29),
                (false, 17, 1),
                (true, 80, 7),
                (false, 7, 1),
                (true, 1000, 137),
                (false, 80, 7),
                (true, 1000, 29),
            ] {
                if restore {
                    rig.track_mut(first)
                        .effects
                        .insert(place, slot.take().unwrap());
                    if hosted {
                        rig.project.plugins.push(binding.clone());
                    }
                } else {
                    slot = Some(rig.remove_effect(fixed));
                    rig.project.plugins.clear();
                }
                controller.set_project(&rig.project, &rig.pool);
                audio.extend(guarded_run(&mut processor, frames, block));
                observe_and_retire(&controller);
            }
            let maximum = left(&audio[4000 * 2..])
                .windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                maximum < 0.04,
                "rapid restored tone step {maximum}, hosted {hosted}, place {place}"
            );
            // The final owner must still apply its full requested delay.
            let total = 96 + if hosted { 32 } else { 48 };
            assert_eq!(controller.latency_frames(), total);
            let final_frame = audio.len() / 2 - 1;
            let expected = 0.25
                * (std::f64::consts::TAU * (final_frame - total as usize) as f64 / 96.0).sin()
                    as f32;
            assert!((audio[audio.len() - 2] - expected).abs() < 1e-6);
            controller.set_project(&rig.project, &rig.pool);
            guarded_run(&mut processor, 137, 137);
            observe_and_retire(&controller);
            if hosted {
                let creates = factory.0.creates.load(std::sync::atomic::Ordering::Relaxed);
                assert_eq!(
                    factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
                    creates - 1
                );
            }
        }
    }
}

#[test]
fn utility_effects_r4_restoration_keeps_serial_route_compensation_cancelling() {
    for hosted in [false, true] {
        for place in 0..=2 {
            for (mix, enabled) in [(1.0, true), (0.5, true), (0.0, true), (1.0, false)] {
                let mut rig = Rig::new();
                let (first, second) = (rig.track(), rig.track());
                let tone = sine(RATE, 173.0, 1.0);
                for (track, sample) in [
                    (first, tone.clone()),
                    (second, crate::support::inverted(&tone)),
                ] {
                    let channel = rig.channel_on(sample, track);
                    rig.channel_mut(channel).volume = 0.5;
                    rig.steps(channel, &[0]);
                }
                rig.effect(first, matrix(1.0, 1.0));
                rig.effect(first, matrix(1.0, 1.0));
                let fixed = rig.effect(
                    first,
                    if hosted {
                        EffectKind::Balance.default_params()
                    } else {
                        idle_limiter(1.0)
                    },
                );
                rig.effect_mut(fixed).mix = mix;
                rig.effect_mut(fixed).enabled = enabled;
                let mut slot = Some(rig.remove_effect(fixed));
                rig.track_mut(first)
                    .effects
                    .insert(place, slot.take().unwrap());
                let binding = windfall_project::PluginBinding {
                    sidechain_input: None,
                    auxiliary_inputs: Vec::new(),
                    capabilities: Default::default(),
                    wrapper: Default::default(),
                    target: windfall_project::PluginTarget::Effect { effect: fixed },
                    format: "clap".into(),
                    path: "r4-cancelling-delay.clap".into(),
                    id: "r4-delay".into(),
                    name: "Prepared delay".into(),
                    state: vec![],
                    parameters: vec![],
                };
                if hosted {
                    rig.project.plugins.push(binding.clone());
                    rig.pool
                        .set_plugin_factory(std::sync::Arc::new(R3DelayFactory::default()));
                }
                let (mut processor, controller) = rig.processor(RATE);
                controller.play();
                assert!(peak(&guarded_run(&mut processor, 4092, 137)) < 1e-6);
                for (restore, frames, block) in [
                    (false, 80, 7),
                    (true, 400, 29),
                    (false, 17, 1),
                    (true, 80, 7),
                    (false, 7, 1),
                    (true, 1000, 137),
                    (false, 1000, 29),
                    (true, 1000, 137),
                ] {
                    if restore {
                        rig.track_mut(first)
                            .effects
                            .insert(place, slot.take().unwrap());
                        if hosted {
                            rig.project.plugins.push(binding.clone());
                        }
                    } else {
                        slot = Some(rig.remove_effect(fixed));
                        rig.project.plugins.clear();
                    }
                    controller.set_project(&rig.project, &rig.pool);
                    let residual = peak(&guarded_run(&mut processor, frames, block));
                    assert!(
                        residual < 1e-6,
                        "restore PDC residual {residual}, hosted {hosted}, place {place}, mix {mix}, enabled {enabled}, restore {restore}, frames {frames}"
                    );
                    observe_and_retire(&controller);
                }
            }
        }
    }
}

#[test]
fn utility_effects_r4_overlapping_restorations_keep_each_serial_splice_clock() {
    let mut rig = Rig::new();
    let (first, second) = (rig.track(), rig.track());
    let tone = sine(RATE, 173.0, 1.0);
    for (track, sample) in [
        (first, tone.clone()),
        (second, crate::support::inverted(&tone)),
    ] {
        let channel = rig.channel_on(sample, track);
        rig.channel_mut(channel).volume = 0.5;
        rig.steps(channel, &[0]);
    }
    let limiter = rig.effect(first, idle_limiter(1.0));
    rig.effect(first, matrix(1.0, 1.0));
    let plugin = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = r4_binding(plugin);
    let stats = std::sync::Arc::new(R4Owners::default());
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
    rig.project.plugins.push(binding.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    assert!(peak(&guarded_run(&mut processor, 4092, 137)) < 1e-6);
    let limiter_slot = rig.remove_effect(limiter);
    controller.set_project(&rig.project, &rig.pool);
    assert!(peak(&guarded_run(&mut processor, 80, 7)) < 1e-6);
    let plugin_slot = rig.remove_effect(plugin);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert!(peak(&guarded_run(&mut processor, 17, 1)) < 1e-6);
    rig.track_mut(first).effects.insert(0, limiter_slot);
    rig.track_mut(first).effects.insert(2, plugin_slot);
    rig.project.plugins.push(binding);
    controller.set_project(&rig.project, &rig.pool);
    let residual = peak(&guarded_run(&mut processor, 1000, 29));
    assert!(
        residual < 1e-6,
        "overlapping restored serial clocks residual {residual}"
    );
    assert_eq!(controller.latency_frames(), 176);
    observe_and_retire(&controller);
    controller.set_project(&rig.project, &rig.pool);
    assert!(peak(&guarded_run(&mut processor, 137, 137)) < 1e-6);
    observe_and_retire(&controller);
    use std::sync::atomic::Ordering::Relaxed;
    assert_eq!(stats.creates.load(Relaxed), 2);
    assert_eq!(stats.owners[0].drops.load(Relaxed), 1);
    assert_eq!(stats.owners[1].drops.load(Relaxed), 0);
}

#[derive(Debug, Default)]
struct R4OwnerStats {
    processes: std::sync::atomic::AtomicUsize,
    last_parameter: std::sync::atomic::AtomicU32,
    drops: std::sync::atomic::AtomicUsize,
}
#[derive(Debug)]
struct R4Owners {
    latency_override: std::sync::atomic::AtomicUsize,
    creates: std::sync::atomic::AtomicUsize,
    revision: std::sync::atomic::AtomicU64,
    owners: [R4OwnerStats; 64],
}
impl Default for R4Owners {
    fn default() -> Self {
        Self {
            latency_override: Default::default(),
            creates: Default::default(),
            revision: Default::default(),
            owners: std::array::from_fn(|_| R4OwnerStats::default()),
        }
    }
}
#[derive(Debug)]
struct R4Factory(std::sync::Arc<R4Owners>);
impl windfall_engine::plugins::PluginFactory for R4Factory {
    fn revision(&self) -> u64 {
        self.0.revision.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn effect(
        &self,
        binding: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedEffect>, String> {
        let owner = self
            .0
            .creates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(owner < self.0.owners.len());
        Ok(Box::new(R4HostedDelay {
            ring: [[0.0; 2]; 64],
            write: 0,
            delay: match self
                .0
                .latency_override
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                0 => binding.state.first().copied().map_or(32, usize::from),
                delay => delay,
            },
            owner,
            stats: self.0.clone(),
        }))
    }
    fn instrument(
        &self,
        _: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedInstrument>, String> {
        Err("effect fixture".into())
    }
}
struct R4HostedDelay {
    ring: [[f32; 2]; 64],
    write: usize,
    delay: usize,
    owner: usize,
    stats: std::sync::Arc<R4Owners>,
}
impl Drop for R4HostedDelay {
    fn drop(&mut self) {
        self.stats.owners[self.owner]
            .drops
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
impl windfall_engine::plugins::HostedEffect for R4HostedDelay {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.stats.owners[self.owner]
            .processes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (left, right) in left.iter_mut().zip(right) {
            let old = self.ring[self.write];
            self.ring[self.write] = [*left, *right];
            self.write = (self.write + 1) % self.delay;
            [*left, *right] = old;
        }
    }
    fn set_param(&mut self, id: u32, value: f32) {
        assert_eq!(id, 7);
        self.stats.owners[self.owner]
            .last_parameter
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
    fn set_tempo(&mut self, _: f32) {}
    fn latency(&self) -> usize {
        self.delay
    }
    fn tail(&self) -> usize {
        self.delay
    }
}
fn r4_binding(effect: windfall_project::EffectId) -> windfall_project::PluginBinding {
    windfall_project::PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
        target: windfall_project::PluginTarget::Effect { effect },
        format: "clap".into(),
        path: "r4-owner-delay.clap".into(),
        id: "r4-owner-delay".into(),
        name: "Prepared delay".into(),
        state: vec![32],
        parameters: vec![windfall_project::PluginParameter {
            id: 7,
            name: "Probe".into(),
            min: 0.0,
            max: 1.0,
            value: 0.25,
            stepped: false,
            read_only: false,
            automatable: true,
        }],
    }
}

#[test]
fn utility_effects_r4_revised_native_restoration_routes_controls_and_automation_only_to_current_owner()
 {
    use std::sync::atomic::Ordering::Relaxed;
    for place in 0..=2 {
        let mut rig = Rig::new();
        let first = rig.track();
        let lane = rig.playlist_track();
        rig.audio_clip(lane, sine(RATE, 500.0, 1.0), first, 0, 1920);
        rig.track_mut(first).volume = 0.5;
        rig.effect(first, matrix(1.0, 1.0));
        rig.effect(first, matrix(1.0, 1.0));
        let id = rig.effect(first, EffectKind::Balance.default_params());
        let slot = rig.remove_effect(id);
        rig.track_mut(first).effects.insert(place, slot);
        let binding = r4_binding(id);
        rig.project.plugins.push(binding.clone());
        let stats = std::sync::Arc::new(R4Owners::default());
        rig.pool
            .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
        let (mut processor, controller) = rig.song_processor(RATE);
        controller.play();
        guarded_run(&mut processor, 4092, 137);
        let slot = rig.remove_effect(id);
        rig.project.plugins.clear();
        controller.set_project(&rig.project, &rig.pool);
        let mut audio = guarded_run(&mut processor, 80, 7);
        let mut restored = binding;
        restored.state = vec![64];
        restored.parameters[0].value = 0.5;
        stats.revision.store(1, Relaxed);
        rig.track_mut(first).effects.insert(place, slot);
        rig.project.plugins.push(restored);
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(guarded_run(&mut processor, 7, 7));
        assert_eq!(stats.creates.load(Relaxed), 2);
        assert_eq!(
            f32::from_bits(stats.owners[0].last_parameter.load(Relaxed)),
            0.25
        );
        assert_eq!(
            f32::from_bits(stats.owners[1].last_parameter.load(Relaxed)),
            0.5
        );
        assert_eq!(stats.owners[0].drops.load(Relaxed), 0);
        // A second control plan and a live automation lane share the restored
        // id. Neither is allowed to control the still audible old owner.
        rig.project.plugins[0].parameters[0].value = 0.625;
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(guarded_run(&mut processor, 7, 7));
        assert_eq!(
            f32::from_bits(stats.owners[1].last_parameter.load(Relaxed)),
            0.625
        );
        let automation = rig.automation(
            AutomationTarget::EffectParam {
                track: first,
                effect: id,
                param: 0,
            },
            &[(0, 0.75)],
        );
        let lane = rig.playlist_track();
        rig.automation_clip(lane, automation, 0, 1920);
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(guarded_run(&mut processor, 80, 7));
        assert_eq!(
            f32::from_bits(stats.owners[0].last_parameter.load(Relaxed)),
            0.25
        );
        assert_eq!(
            f32::from_bits(stats.owners[1].last_parameter.load(Relaxed)),
            0.75
        );
        audio.extend(guarded_run(&mut processor, 1000, 29));
        let step = left(&audio)
            .windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            step < 0.04,
            "revised restored tone step {step}, place {place}"
        );
        assert_eq!(controller.latency_frames(), 160);
        assert_eq!(stats.owners[0].drops.load(Relaxed), 0);
        observe_and_retire(&controller);
        controller.set_project(&rig.project, &rig.pool);
        guarded_run(&mut processor, 137, 137);
        observe_and_retire(&controller);
        assert_eq!(stats.creates.load(Relaxed), 2);
        assert_eq!(stats.owners[0].drops.load(Relaxed), 1);
        assert_eq!(stats.owners[1].drops.load(Relaxed), 0);
    }
}

#[test]
fn utility_effects_r4_superseded_restores_cannot_accumulate_or_activate_native_owners() {
    use std::sync::atomic::Ordering::Relaxed;
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(sine(RATE, 500.0, 1.0), first);
    rig.channel_mut(channel).volume = 0.5;
    rig.steps(channel, &[0]);
    let id = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = r4_binding(id);
    rig.project.plugins.push(binding.clone());
    let stats = std::sync::Arc::new(R4Owners::default());
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    guarded_run(&mut processor, 4092, 137);
    let mut slot = Some(rig.remove_effect(id));
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 80, 7);
    for _ in 0..40 {
        rig.track_mut(first).effects.insert(0, slot.take().unwrap());
        rig.project.plugins.push(binding.clone());
        controller.set_project(&rig.project, &rig.pool);
        slot = Some(rig.remove_effect(id));
        rig.project.plugins.clear();
        // Supersede the fresh plan before it is heard. It must not join the
        // outgoing lineage or inherit that owner's native authority.
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(guarded_run(&mut processor, 1, 1));
        observe_and_retire(&controller);
        let count = stats.creates.load(Relaxed);
        let dropped: usize = stats
            .owners
            .iter()
            .map(|owner| owner.drops.load(Relaxed))
            .sum();
        assert_eq!(count - dropped, 1);
        assert!(
            stats.owners[1..count]
                .iter()
                .all(|owner| owner.processes.load(Relaxed) == 0)
        );
    }
    audio.extend(guarded_run(&mut processor, 1000, 29));
    let step = left(&audio)
        .windows(2)
        .map(|p| (p[1] - p[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        step < 0.04,
        "speculative restoration interrupted departure: {step}"
    );
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 137, 137);
    observe_and_retire(&controller);
    assert_eq!(stats.creates.load(Relaxed), 41);
    assert!(
        stats.owners[..41]
            .iter()
            .all(|owner| owner.drops.load(Relaxed) == 1)
    );
}

fn r5_revision_only_restoration(cancel: bool, revision_frames: usize) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut rig = Rig::new();
    let first = rig.track();
    let tone = sine(RATE, 173.0, 1.0);
    let up = rig.channel_on(tone.clone(), first);
    rig.channel_mut(up).volume = 0.5;
    rig.steps(up, &[0]);
    if cancel {
        let second = rig.track();
        let down = rig.channel_on(crate::support::inverted(&tone), second);
        rig.channel_mut(down).volume = 0.5;
        rig.steps(down, &[0]);
    }
    let id = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = r4_binding(id);
    rig.project.plugins.push(binding.clone());
    let stats = std::sync::Arc::new(R4Owners::default());
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    guarded_run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(id);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 80, 7);
    rig.track_mut(first).effects.insert(0, slot);
    rig.project.plugins.push(binding.clone());
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 80, 7));
    let unchanged = rig.project.clone();
    stats.revision.fetch_add(1, Relaxed);
    // Only the existing factory's revision changes. Project, binding state,
    // parameter layout/values, slot id and factory Arc stay exactly the same.
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(rig.project, unchanged);
    let revised = guarded_run(&mut processor, revision_frames, 1);
    if cancel {
        let residual = peak(&revised);
        println!("revision-only restoration residual {residual}");
        assert!(
            residual < 1e-6,
            "revision-only restoration residual {residual}"
        );
    }
    audio.extend(revised);
    observe_and_retire(&controller);
    let slot = rig.remove_effect(id);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 1, 1));
    rig.track_mut(first).effects.insert(0, slot);
    rig.project.plugins.push(binding);
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 1, 1));
    observe_and_retire(&controller);
    let count = stats.creates.load(Relaxed);
    let dropped: usize = stats
        .owners
        .iter()
        .map(|owner| owner.drops.load(Relaxed))
        .sum();
    assert!(
        count - dropped <= 2,
        "restored native live owners {}, expected at most two",
        count - dropped
    );
    audio.extend(guarded_run(&mut processor, 1000, 29));
    if cancel {
        assert!(peak(&audio) < 1e-6);
    } else {
        let step = left(&audio)
            .windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .fold(0.0_f32, f32::max);
        println!("revision-only restoration solo step {step}");
        assert!(step < 0.04, "revision-only restoration solo step {step}");
    }
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 137, 137);
    observe_and_retire(&controller);
    assert_eq!(stats.creates.load(Relaxed), 4);
    assert!(
        stats.owners[..3]
            .iter()
            .all(|owner| owner.drops.load(Relaxed) == 1)
    );
    assert_eq!(stats.owners[3].drops.load(Relaxed), 0);
    assert_eq!(controller.latency_frames(), 80);
}

#[test]
fn utility_effects_r5_revision_only_restoration_keeps_reference_cancellation() {
    r5_revision_only_restoration(true, 400);
}

#[test]
fn utility_effects_r5_revision_only_restoration_keeps_solo_continuity_and_owner_bound() {
    r5_revision_only_restoration(false, 64);
}

#[test]
fn utility_effects_r5_revision_only_remove_restore_before_departure_finishes() {
    r5_revision_only_restoration(true, 64);
}

#[test]
fn utility_effects_r5_pending_revisions_keep_latency_changes_and_precompiled_plans_bounded() {
    use std::sync::atomic::Ordering::Relaxed;
    for place in 0..=2 {
        let mut rig = Rig::new();
        let (first, second) = (rig.track(), rig.track());
        let tone = sine(RATE, 173.0, 1.0);
        for (track, sample) in [
            (first, tone.clone()),
            (second, crate::support::inverted(&tone)),
        ] {
            let channel = rig.channel_on(sample, track);
            rig.channel_mut(channel).volume = 0.5;
            rig.steps(channel, &[0]);
        }
        rig.effect(first, matrix(1.0, 1.0));
        rig.effect(first, matrix(1.0, 1.0));
        let id = rig.effect(first, EffectKind::Balance.default_params());
        let slot = rig.remove_effect(id);
        rig.track_mut(first).effects.insert(place, slot);
        let binding = r4_binding(id);
        rig.project.plugins.push(binding.clone());
        let stats = std::sync::Arc::new(R4Owners::default());
        rig.pool
            .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        assert!(peak(&guarded_run(&mut processor, 4092, 137)) < 1e-6);
        let mut slot = Some(rig.remove_effect(id));
        rig.project.plugins.clear();
        controller.set_project(&rig.project, &rig.pool);
        assert!(peak(&guarded_run(&mut processor, 80, 7)) < 1e-6);
        rig.track_mut(first)
            .effects
            .insert(place, slot.take().unwrap());
        rig.project.plugins.push(binding.clone());
        controller.set_project(&rig.project, &rig.pool);
        assert!(peak(&guarded_run(&mut processor, 80, 7)) < 1e-6);
        let unchanged = rig.project.clone();
        // Compile before each retry, then install after the revision changes.
        // Metadata must snapshot installation identity, not query a past
        // plan's now-mutated factory or trust precompilation's old identity.
        for edit in 0..8 {
            let prepared = windfall_engine::Controller::prepare_project(&rig.project, &rig.pool)
                .expect("the restoration fixture prepares successfully");
            stats
                .latency_override
                .store(if edit % 2 == 0 { 64 } else { 32 }, Relaxed);
            stats.revision.fetch_add(1, Relaxed);
            let mut ready = controller
                .preparation_snapshot()
                .prepare(
                    &rig.project,
                    prepared.sampler_pool(),
                    windfall_engine::ProjectPublicationIntent::Edit,
                )
                .expect("the restoration fixture has ready native units");
            let mut retirement = controller
                .publication(&mut ready)
                .expect("the restoration fixture is still current")
                .install();
            controller.take_retired(&mut retirement);
            assert_eq!(rig.project, unchanged);
            let residual = peak(&guarded_run(&mut processor, 7, 1));
            assert!(
                residual < 1e-6,
                "pending revised latency residual {residual}, place {place}, edit {edit}"
            );
            observe_and_retire(&controller);
            let count = stats.creates.load(Relaxed);
            let dropped: usize = stats
                .owners
                .iter()
                .map(|owner| owner.drops.load(Relaxed))
                .sum();
            assert_eq!(count - dropped, 2);
        }
        // Remove while the original departure is still audible, then restore
        // with the same binding and a revised 64-frame prepared owner.
        slot = Some(rig.remove_effect(id));
        rig.project.plugins.clear();
        controller.set_project(&rig.project, &rig.pool);
        assert!(peak(&guarded_run(&mut processor, 1, 1)) < 1e-6);
        stats.latency_override.store(64, Relaxed);
        stats.revision.fetch_add(1, Relaxed);
        rig.track_mut(first)
            .effects
            .insert(place, slot.take().unwrap());
        rig.project.plugins.push(binding);
        controller.set_project(&rig.project, &rig.pool);
        assert!(peak(&guarded_run(&mut processor, 1000, 29)) < 1e-6);
        controller.set_project(&rig.project, &rig.pool);
        assert!(peak(&guarded_run(&mut processor, 137, 137)) < 1e-6);
        observe_and_retire(&controller);
        let count = stats.creates.load(Relaxed);
        assert_eq!(count, 11);
        assert!(
            stats.owners[..count - 1]
                .iter()
                .all(|owner| owner.drops.load(Relaxed) == 1)
        );
        assert_eq!(stats.owners[count - 1].drops.load(Relaxed), 0);
        assert_eq!(controller.latency_frames(), 160);
    }
}

#[test]
fn utility_effects_r5_superseded_revision_owners_never_process_or_join_departures() {
    use std::sync::atomic::Ordering::Relaxed;
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(sine(RATE, 173.0, 1.0), first);
    rig.channel_mut(channel).volume = 0.5;
    rig.steps(channel, &[0]);
    let id = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = r4_binding(id);
    rig.project.plugins.push(binding.clone());
    let stats = std::sync::Arc::new(R4Owners::default());
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    guarded_run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(id);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 80, 7);
    rig.track_mut(first).effects.insert(0, slot);
    rig.project.plugins.push(binding);
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 80, 7));
    let unchanged = rig.project.clone();
    for _ in 0..20 {
        stats.revision.fetch_add(1, Relaxed);
        controller.set_project(&rig.project, &rig.pool);
        let superseded = stats.creates.load(Relaxed) - 1;
        stats.revision.fetch_add(1, Relaxed);
        controller.set_project(&rig.project, &rig.pool);
        assert_eq!(rig.project, unchanged);
        audio.extend(guarded_run(&mut processor, 1, 1));
        observe_and_retire(&controller);
        assert_eq!(stats.owners[superseded].processes.load(Relaxed), 0);
        assert_eq!(stats.owners[superseded].drops.load(Relaxed), 1);
        let count = stats.creates.load(Relaxed);
        let dropped: usize = stats
            .owners
            .iter()
            .map(|owner| owner.drops.load(Relaxed))
            .sum();
        assert_eq!(count - dropped, 2);
    }
    audio.extend(guarded_run(&mut processor, 1000, 29));
    let step = left(&audio)
        .windows(2)
        .map(|p| (p[1] - p[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(step < 0.04, "repeated revision solo step {step}");
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 137, 137);
    observe_and_retire(&controller);
    assert_eq!(stats.creates.load(Relaxed), 42);
    assert!(
        stats.owners[..41]
            .iter()
            .all(|owner| owner.drops.load(Relaxed) == 1)
    );
    assert_eq!(stats.owners[41].drops.load(Relaxed), 0);
}

#[test]
fn utility_effects_r5_revised_native_id_moves_after_departure_without_new_owner_or_unity_hole() {
    use std::sync::atomic::Ordering::Relaxed;
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(level(RATE, 1.0, 1.0), first);
    rig.steps(channel, &[0]);
    let id = rig.effect(first, EffectKind::Balance.default_params());
    rig.effect(first, matrix(1.0, 1.0));
    let binding = r4_binding(id);
    rig.project.plugins.push(binding.clone());
    let stats = std::sync::Arc::new(R4Owners::default());
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R4Factory(stats.clone())));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    guarded_run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(id);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 80, 7)
            .iter()
            .all(|sample| *sample == 1.0)
    );
    rig.track_mut(first).effects.insert(0, slot);
    rig.project.plugins.push(binding);
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 80, 7);
    stats.revision.fetch_add(1, Relaxed);
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 1000, 29)
            .iter()
            .all(|sample| *sample == 1.0)
    );
    let slot = rig.remove_effect(id);
    rig.track_mut(TrackId::MASTER).effects.push(slot);
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 1000, 137)
            .iter()
            .all(|sample| *sample == 1.0)
    );
    observe_and_retire(&controller);
    assert_eq!(stats.creates.load(Relaxed), 3);
    assert_eq!(stats.owners[0].drops.load(Relaxed), 1);
    assert_eq!(stats.owners[1].drops.load(Relaxed), 1);
    assert_eq!(stats.owners[2].drops.load(Relaxed), 0);
    assert_eq!(controller.latency_frames(), 80);
}

#[test]
fn utility_effects_r3_prepared_native_revision_cannot_be_activated_as_a_departing_owner() {
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(level(RATE, 1.0, 1.0), first);
    rig.steps(channel, &[0]);
    let plugin = rig.effect(first, EffectKind::Balance.default_params());
    rig.project.plugins.push(windfall_project::PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        capabilities: Default::default(),
        wrapper: Default::default(),
        target: windfall_project::PluginTarget::Effect { effect: plugin },
        format: "clap".into(),
        path: "r3-revised-delay.clap".into(),
        id: "r3-delay".into(),
        name: "Prepared delay".into(),
        state: vec![],
        parameters: vec![],
    });
    let factory = std::sync::Arc::new(R3DelayFactory::default());
    rig.pool.set_plugin_factory(factory.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let processes = factory
        .0
        .processes
        .load(std::sync::atomic::Ordering::Relaxed);
    factory.1.store(1, std::sync::atomic::Ordering::Relaxed);
    controller.set_project(&rig.project, &rig.pool);
    rig.remove_effect(plugin);
    rig.project.plugins.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 1000, 7)
            .iter()
            .all(|s| *s == 1.0)
    );
    observe_and_retire(&controller);
    assert_eq!(
        factory.0.creates.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert_eq!(
        factory
            .0
            .processes
            .load(std::sync::atomic::Ordering::Relaxed),
        processes
    );
    controller.set_project(&rig.project, &rig.pool);
    guarded_run(&mut processor, 7, 7);
    observe_and_retire(&controller);
    assert_eq!(
        factory.0.drops.load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

#[test]
fn utility_effects_r3_moved_matrix_departure_keeps_progress_through_factory_changes() {
    let mut rig = Rig::new();
    let first = rig.track();
    let channel = rig.channel_on(sine(RATE, 500.0, 1.0), first);
    rig.channel_mut(channel).volume = 0.5;
    rig.steps(channel, &[0]);
    let id = rig.effect(first, matrix(1.0, 1.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let slot = rig.remove_effect(id);
    rig.track_mut(TrackId::MASTER).effects.push(slot);
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = guarded_run(&mut processor, 1000, 29);
    rig.pool
        .set_plugin_factory(std::sync::Arc::new(R3DelayFactory::default()));
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 1, 1));
    rig.remove_effect(id);
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(guarded_run(&mut processor, 1000, 29));
    let maximum = left(&audio)
        .windows(2)
        .map(|p| (p[1] - p[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(maximum < 0.04, "moved departure interrupted: {maximum}");
}

#[test]
fn utility_effects_r3_reference_change_preserves_constant_unity() {
    let mut rig = Rig::new();
    let tracks = [rig.track(), rig.track(), rig.track()];
    for track in tracks {
        let channel = rig.channel_on(level(RATE, 1.0, 1.0), track);
        rig.steps(channel, &[0]);
    }
    rig.effect(tracks[0], matrix(1.0, 1.0));
    let b = rig.effect(tracks[1], matrix(0.0, 0.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    rig.effect_mut(b).params = matrix(2.0, 2.0);
    controller.set_project(&rig.project, &rig.pool);
    let audio = run(&mut processor, 1000, 29);
    let minimum = audio.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(
        audio.iter().all(|sample| *sample == 3.0),
        "reference-switch unity minimum {minimum}"
    );
    assert_eq!(controller.latency_frames(), 96);
}

#[test]
fn utility_effects_r3_short_history_edits_keep_wet_dry_and_pdc_synchronized() {
    for block in [1, 7, 137] {
        for (mix, enabled) in [(1.0, true), (0.5, true), (0.0, true), (1.0, false)] {
            let mut rig = Rig::new();
            let (first, second) = (rig.track(), rig.track());
            let tone = sine(RATE, 173.0, 1.0);
            let up = rig.channel_on(tone.clone(), first);
            let down = rig.channel_on(crate::support::inverted(&tone), second);
            rig.steps(up, &[0]);
            rig.steps(down, &[0]);
            let id = rig.effect(first, matrix(0.0, 0.0));
            rig.effect_mut(id).mix = mix;
            rig.effect_mut(id).enabled = enabled;
            let (mut processor, controller) = rig.processor(RATE);
            for reset in [false, true] {
                if reset {
                    controller.stop();
                    run(&mut processor, 3000, block);
                    rig.effect_mut(id).params = matrix(0.0, 0.0);
                    controller.set_project(&rig.project, &rig.pool);
                    controller.seek(0.0);
                }
                controller.play();
                let mut audio = guarded_run(&mut processor, 64, block);
                for (ms, frames) in [(5.0, 100), (50.0, 3000), (2.0, 500)] {
                    rig.effect_mut(id).params = matrix(ms, ms);
                    controller.set_project(&rig.project, &rig.pool);
                    audio.extend(guarded_run(&mut processor, frames, block));
                }
                assert!(
                    peak(&audio) < 1e-6,
                    "short-history PDC {}, block {block}, mix {mix}, enabled {enabled}, reset {reset}",
                    peak(&audio)
                );
            }
            rig.track_mut(second).muted = true;
            controller.set_project(&rig.project, &rig.pool);
            assert!(peak(&run(&mut processor, 1000, block)) > 0.4);
        }
    }
}

#[test]
fn utility_effects_r3_repeated_reference_switches_keep_retained_input_unity() {
    for primed in [64, 4092] {
        let mut rig = Rig::new();
        let tracks = [rig.track(), rig.track(), rig.track()];
        for track in tracks {
            let channel = rig.channel_on(level(RATE, 1.0, 1.0), track);
            rig.steps(channel, &[0]);
        }
        let a = rig.effect(tracks[0], matrix(0.0, 0.0));
        let b = rig.effect(tracks[1], matrix(0.0, 0.0));
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        run(&mut processor, primed, 137);
        for (id, ms, frames, block) in [
            (a, 1.0, 100, 7),
            (b, 2.0, 17, 1),
            (a, 5.0, 37, 7),
            (b, 50.0, 29, 1),
            (a, 0.0, 61, 7),
            (b, 3.0, 3000, 137),
            (a, 4.0, 1000, 29),
        ] {
            rig.effect_mut(id).params = matrix(ms, ms);
            controller.set_project(&rig.project, &rig.pool);
            let mut out = [0.0; 137 * 2];
            let mut remaining = frames;
            while remaining > 0 {
                let count = remaining.min(block);
                let out = &mut out[..count * 2];
                assert_eq!(
                    crate::realtime::allocator_calls(|| processor.process(out)),
                    0
                );
                let minimum = out.iter().copied().fold(f32::INFINITY, f32::min);
                assert!(
                    out.iter().all(|sample| (*sample - 3.0).abs() < 1e-6),
                    "reference unity {minimum}, primed {primed}, target {ms}"
                );
                remaining -= count;
            }
            observe_and_retire(&controller);
        }
    }
}

#[test]
fn utility_effects_r3_source_and_matrix_route_changes_do_not_expose_empty_history() {
    let mut rig = Rig::new();
    let (first, plain, bus) = (rig.track(), rig.track(), rig.track());
    let up = rig.channel_on(level(RATE, 1.0, 1.0), first);
    let direct = rig.channel_on(level(RATE, 1.0, 1.0), plain);
    rig.steps(up, &[0]);
    rig.steps(direct, &[0]);
    let a = rig.effect(first, matrix(0.0, 0.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 64, 7);
    rig.effect_mut(a).params = matrix(50.0, 50.0);
    controller.set_project(&rig.project, &rig.pool);
    assert!(run(&mut processor, 100, 7).iter().all(|s| *s == 2.0));
    let added = rig.channel_on(level(RATE, 1.0, 1.0), bus);
    rig.steps(added, &[0]);
    let b = rig.effect(bus, matrix(0.0, 50.0));
    controller.set_project(&rig.project, &rig.pool);
    // A newly added sampler starts at its next pattern trigger. Seek/replay
    // starts the added source. Prime constant audio after the transport fade:
    // earlier silence in the real source is legitimate delayed input, whereas
    // empty history in a newly constructed compensation stage is not.
    controller.stop();
    run(&mut processor, 3000, 137);
    rig.effect_mut(a).params = matrix(0.0, 0.0);
    rig.effect_mut(b).params = matrix(0.0, 0.0);
    controller.set_project(&rig.project, &rig.pool);
    controller.seek(0.0);
    controller.play();
    let initial = guarded_run(&mut processor, 4092, 7);
    assert!(initial[6000..].iter().all(|s| *s == 3.0));
    rig.effect_mut(b).params = matrix(50.0, 50.0);
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 100, 7)
            .iter()
            .all(|s| *s == 3.0)
    );
    let extra = rig.effect(bus, matrix(1.0, 1.0));
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 100, 7)
            .iter()
            .all(|s| (*s - 3.0).abs() < 1e-6)
    );
    let moved = rig.remove_effect(extra);
    rig.track_mut(first).effects.push(moved);
    rig.channel_mut(direct).mixer_track = bus;
    controller.set_project(&rig.project, &rig.pool);
    let audio = guarded_run(&mut processor, 3000, 137);
    assert!(
        audio.iter().all(|s| (*s - 3.0).abs() < 1e-6),
        "moved source/matrix unity minimum {}",
        audio.iter().copied().fold(f32::INFINITY, f32::min)
    );
    rig.remove_effect(extra);
    controller.set_project(&rig.project, &rig.pool);
    assert!(
        guarded_run(&mut processor, 1000, 29)
            .iter()
            .all(|s| (*s - 3.0).abs() < 1e-6)
    );
    rig.project.channels.retain(|channel| channel.id != added);
    controller.set_project(&rig.project, &rig.pool);
    let audio = guarded_run(&mut processor, 3500, 137);
    assert!(audio.iter().all(|s| (2.0 - 1e-6..=3.0 + 1e-6).contains(s)));
    assert!(audio[6000..].iter().all(|s| (*s - 2.0).abs() < 1e-6));
}

#[test]
fn utility_effects_r2_live_asymmetric_insertion_primes_both_outputs() {
    for block in [1, 137, 511] {
        let mut rig = Rig::new();
        let track = rig.track();
        let channel = rig.channel_on(level(RATE, 1.0, 1.0), track);
        rig.steps(channel, &[0]);
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        run(&mut processor, 4092, block);
        rig.effect(track, matrix(0.0, 50.0));
        controller.set_project(&rig.project, &rig.pool);
        assert_eq!(controller.latency_frames(), 0);
        let audio = run(&mut processor, 4096, block);
        for (frame, sample) in right(&audio).iter().enumerate() {
            assert_eq!(
                *sample, 1.0,
                "unprimed live insertion at {frame}, block {block}"
            );
        }
    }
}

#[test]
fn utility_effects_r2_longer_delay_during_insertion_extends_output_priming() {
    let mut rig = Rig::new();
    let track = rig.track();
    let channel = rig.channel_on(level(RATE, 1.0, 1.0), track);
    rig.steps(channel, &[0]);
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let id = rig.effect(track, matrix(0.0, 5.0));
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = run(&mut processor, 100, 7);
    rig.effect_mut(id).params = matrix(0.0, 50.0);
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(run(&mut processor, 4000, 137));
    assert!(
        audio.iter().all(|sample| *sample == 1.0),
        "longer insertion target exposed an unprimed output"
    );
    assert_eq!(controller.latency_frames(), 0);
}

#[test]
fn utility_effects_r2_insertion_priming_extension_keeps_shared_pdc_in_step() {
    let mut rig = Rig::new();
    let (first, second) = (rig.track(), rig.track());
    let tone = sine(RATE, 173.0, 0.25);
    let up = rig.channel_on(tone.clone(), first);
    let down = rig.channel_on(crate::support::inverted(&tone), second);
    rig.steps(up, &[0]);
    rig.steps(down, &[0]);
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    run(&mut processor, 4092, 137);
    let id = rig.effect(first, matrix(0.0, 5.0));
    controller.set_project(&rig.project, &rig.pool);
    let mut audio = run(&mut processor, 100, 7);
    rig.effect_mut(id).params = matrix(2.0, 50.0);
    controller.set_project(&rig.project, &rig.pool);
    audio.extend(run(&mut processor, 4000, 137));
    assert!(
        peak(&left(&audio)) < 1e-6,
        "extended insertion wait desynchronized shared PDC"
    );
    assert!(
        peak(&right(&audio)) > 0.2,
        "the intentional stereo delay must eventually be heard"
    );
    assert_eq!(controller.latency_frames(), 96);
}

#[test]
fn utility_effects_r2_zero_to_delay_keeps_all_slot_policies_cancelling() {
    for (mix, enabled) in [(1.0, true), (0.5, true), (0.0, true), (1.0, false)] {
        let mut rig = Rig::new();
        let first = rig.track();
        let second = rig.track();
        let tone = sine(RATE, 173.0, 0.25);
        let up = rig.channel_on(tone.clone(), first);
        let down = rig.channel_on(crate::support::inverted(&tone), second);
        rig.steps(up, &[0]);
        rig.steps(down, &[0]);
        let effect = rig.effect(first, matrix(0.0, 0.0));
        rig.effect_mut(effect).mix = mix;
        rig.effect_mut(effect).enabled = enabled;
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let mut audio = run(&mut processor, 4092, 137);
        rig.effect_mut(effect).params = matrix(2.0, 2.0);
        controller.set_project(&rig.project, &rig.pool);
        assert_eq!(controller.latency_frames(), 96);
        audio.extend(run(&mut processor, 1000, 29));
        assert!(
            peak(&audio) < 1e-6,
            "zero-to-delay PDC residual {}, mix {mix}, enabled {enabled}",
            peak(&audio)
        );
    }
}

#[test]
fn utility_effects_r2_upstream_edit_reaches_pdc_after_the_downstream_delay() {
    let mut rig = Rig::new();
    let first = rig.track();
    let second = rig.track();
    let tone = sine(RATE, 500.0, 0.25);
    let up = rig.channel_on(tone.clone(), first);
    let down = rig.channel_on(crate::support::inverted(&tone), second);
    rig.steps(up, &[0]);
    rig.steps(down, &[0]);
    let upstream = rig.effect(first, matrix(1.0, 1.0));
    rig.effect(first, matrix(1.0, 1.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut audio = run(&mut processor, 4092, 137);
    rig.effect_mut(upstream).params = matrix(2.0, 2.0);
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(controller.latency_frames(), 144);
    audio.extend(run(&mut processor, 1000, 29));
    assert!(
        peak(&audio) < 1e-6,
        "downstream PDC residual {}",
        peak(&audio)
    );
}

#[test]
fn utility_effects_r2_overlapping_serial_edits_cancel_across_tracks_and_blocks() {
    for routed in [false, true] {
        for (mix, enabled) in [(1.0, true), (0.5, true), (0.0, true), (1.0, false)] {
            let mut rig = Rig::new();
            let (first, second, bus) = (rig.track(), rig.track(), rig.track());
            if routed {
                rig.track_mut(first).output = Some(bus);
            }
            let tone = sine(RATE, 500.0, 0.25);
            let up = rig.channel_on(tone.clone(), first);
            let down = rig.channel_on(crate::support::inverted(&tone), second);
            rig.steps(up, &[0]);
            rig.steps(down, &[0]);
            let upstream = rig.effect(first, matrix(1.0, 1.0));
            let downstream = rig.effect(if routed { bus } else { first }, matrix(1.0, 1.0));
            for id in [upstream, downstream] {
                rig.effect_mut(id).mix = mix;
                rig.effect_mut(id).enabled = enabled;
            }
            let (mut processor, controller) = rig.processor(RATE);
            controller.play();
            let mut audio = run(&mut processor, 4092, 137);
            for (a, b, frames, block) in [
                (2.0, 1.0, 120, 29),
                (0.5, 2.0, 37, 1),
                (1.0, 0.0, 17, 7),
                (0.0, 0.0, 61, 3),
                (0.75, 1.5, 3, 1),
                (2.0, 3.0, 1000, 137),
            ] {
                rig.effect_mut(upstream).params = matrix(a, a);
                rig.effect_mut(downstream).params = matrix(b, b);
                controller.set_project(&rig.project, &rig.pool);
                assert_eq!(controller.latency_frames(), ((a + b) * 48.0).round() as u32);
                audio.extend(run(&mut processor, frames, block));
            }
            assert!(
                peak(&audio) < 1e-6,
                "overlapping serial PDC residual {}, routed {routed}, mix {mix}, enabled {enabled}",
                peak(&audio)
            );
            // Cancellation must come from matching audible transfers. The
            // settled chain itself still produces the requested 5 ms delay.
            rig.track_mut(second).muted = true;
            controller.set_project(&rig.project, &rig.pool);
            let mut solo = run(&mut processor, 1000, 29);
            assert!(peak(&solo) > 0.2);
            controller.stop();
            run(&mut processor, 1000, 7);
            controller.seek(0.0);
            controller.play();
            solo = run(&mut processor, 1000, 137);
            let reference = rig.play(RATE, 1000, 1);
            assert_eq!(
                solo, reference,
                "reset/replay must reuse the final settled chain"
            );
        }
    }
}

#[test]
fn utility_effects_r2_matrix_edits_stay_aligned_through_a_fixed_limiter() {
    let mut rig = Rig::new();
    let (first, second) = (rig.track(), rig.track());
    let tone = sine(RATE, 500.0, 0.25);
    let up = rig.channel_on(tone.clone(), first);
    let down = rig.channel_on(crate::support::inverted(&tone), second);
    rig.steps(up, &[0]);
    rig.steps(down, &[0]);
    let matrix_id = rig.effect(first, matrix(0.0, 0.0));
    rig.effect(first, idle_limiter(1.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut audio = run(&mut processor, 4092, 137);
    for (ms, frames) in [(2.0, 120), (0.5, 37), (1.0, 1000)] {
        rig.effect_mut(matrix_id).params = matrix(ms, ms);
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(run(&mut processor, frames, 29));
    }
    assert!(
        peak(&audio) < 1e-6,
        "limiter downstream PDC residual {}",
        peak(&audio)
    );
}

#[test]
fn utility_effects_r2_matrix_splices_preserve_existing_fixed_delay_history() {
    for place in 0..=2 {
        let mut rig = Rig::new();
        let (first, second) = (rig.track(), rig.track());
        let tone = sine(RATE, 173.0, 0.25);
        let up = rig.channel_on(tone.clone(), first);
        let down = rig.channel_on(crate::support::inverted(&tone), second);
        rig.steps(up, &[0]);
        rig.steps(down, &[0]);
        rig.effect(first, idle_limiter(1.0));
        rig.effect(first, idle_limiter(1.0));
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let mut audio = run(&mut processor, 4092, 137);
        let matrix_id = rig.effect(first, matrix(2.0, 2.0));
        let slot = rig.remove_effect(matrix_id);
        rig.track_mut(first).effects.insert(place, slot);
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(run(&mut processor, 1000, 29));
        assert!(
            peak(&audio) < 1e-6,
            "matrix insertion at {place} residual {}",
            peak(&audio)
        );
        assert_eq!(controller.latency_frames(), 192);
        rig.remove_effect(matrix_id);
        controller.set_project(&rig.project, &rig.pool);
        audio.extend(run(&mut processor, 1000, 7));
        assert!(
            peak(&audio) < 1e-6,
            "matrix removal at {place} residual {}",
            peak(&audio)
        );
        assert_eq!(controller.latency_frames(), 96);
    }
}

#[test]
fn utility_effects_r2_sampler_matrix_and_hosted_delay_keep_pdc_and_plugin_ownership() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory};
    use windfall_project::{PluginBinding, PluginTarget};

    struct HostedDelay {
        ring: [[f32; 2]; 32],
        write: usize,
    }
    impl HostedEffect for HostedDelay {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            for (left, right) in left.iter_mut().zip(right) {
                let old = self.ring[self.write];
                self.ring[self.write] = [*left, *right];
                self.write = (self.write + 1) % 32;
                [*left, *right] = old;
            }
        }
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            32
        }
        fn tail(&self) -> usize {
            32
        }
    }
    #[derive(Debug)]
    struct Factory(AtomicUsize);
    impl PluginFactory for Factory {
        fn effect(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(HostedDelay {
                ring: [[0.0; 2]; 32],
                write: 0,
            }))
        }
        fn instrument(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            Err("effect fixture".into())
        }
    }
    for enabled in [true, false] {
        let mut rig = Rig::new();
        let (first, second) = (rig.track(), rig.track());
        let tone = sine(RATE, 173.0, 0.25);
        let up = rig.channel_on(tone.clone(), first);
        let down = rig.channel_on(crate::support::inverted(&tone), second);
        rig.steps(up, &[0]);
        rig.steps(down, &[0]);
        let matrix_id = rig.effect(first, matrix(0.0, 0.0));
        rig.effect_mut(matrix_id).mix = 0.5;
        let plugin = rig.effect(first, EffectKind::Balance.default_params());
        rig.effect_mut(plugin).enabled = enabled;
        rig.project.plugins.push(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            capabilities: Default::default(),
            wrapper: Default::default(),
            target: PluginTarget::Effect { effect: plugin },
            format: "clap".into(),
            path: "prepared-delay-fixture.clap".into(),
            id: "utility-delay".into(),
            name: "Prepared delay".into(),
            state: vec![],
            parameters: vec![],
        });
        let factory = Arc::new(Factory(AtomicUsize::new(0)));
        rig.pool.set_plugin_factory(factory.clone());
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let checked = |processor: &mut windfall_engine::Processor, frames: usize, block: usize| {
            let mut out = [0.0; 137 * 2];
            let mut maximum = 0.0_f32;
            let mut remaining = frames;
            while remaining > 0 {
                let count = remaining.min(block);
                let audio = &mut out[..count * 2];
                assert_eq!(
                    crate::realtime::allocator_calls(|| processor.process(audio)),
                    0
                );
                maximum = maximum.max(peak(audio));
                remaining -= count;
            }
            maximum
        };
        let mut residual = checked(&mut processor, 4092, 137);
        for (ms, frames, block) in [(2.0, 120, 29), (0.5, 37, 1), (1.0, 1000, 137)] {
            rig.effect_mut(matrix_id).params = matrix(ms, ms);
            controller.set_project(&rig.project, &rig.pool);
            assert_eq!(controller.latency_frames(), (ms * 48.0).round() as u32 + 32);
            residual = residual.max(checked(&mut processor, frames, block));
            observe_and_retire(&controller);
        }
        assert!(
            residual < 1e-6,
            "hosted delay PDC residual {residual}, enabled {enabled}"
        );
        assert_eq!(
            factory.0.load(Ordering::Relaxed),
            1,
            "delay edits must reuse the plugin"
        );
    }
}

#[test]
fn utility_effects_r2_causal_history_edits_splices_and_retirement_never_use_the_allocator() {
    let mut rig = Rig::new();
    let (first, second, bus) = (rig.track(), rig.track(), rig.track());
    rig.track_mut(first).output = Some(bus);
    let tone = sine(RATE, 500.0, 1.0);
    let up = rig.channel_on(tone.clone(), first);
    let down = rig.channel_on(crate::support::inverted(&tone), second);
    rig.steps(up, &[0]);
    rig.steps(down, &[0]);
    let upstream = rig.effect(first, matrix(0.0, 0.0));
    let downstream = rig.effect(bus, matrix(0.0, 0.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut out = [0.0; 1024 * 2];
    let mut calls = 0;
    for _ in 0..5 {
        calls += crate::realtime::allocator_calls(|| processor.process(&mut out));
    }
    let mut inserted = None;
    for edit in 0..80 {
        for (index, id) in [upstream, downstream].into_iter().enumerate() {
            let delay = ((edit * 37 + index * 17) % 2401) as f32 / 48.0;
            rig.effect_mut(id).params = matrix(delay, delay);
            rig.effect_mut(id).mix = [0.0, 0.5, 1.0][edit % 3];
            rig.effect_mut(id).enabled = edit % 5 != 0;
        }
        if edit == 5 {
            inserted = Some(rig.effect(first, matrix(0.0, 50.0)));
        }
        if edit == 45 {
            rig.remove_effect(inserted.take().unwrap());
        }
        controller.set_project(&rig.project, &rig.pool);
        if edit == 20 {
            controller.stop();
        }
        if edit == 21 {
            controller.seek(0.0);
            controller.play();
        }
        let frames = [1, 7, 137, 511, 1024][edit % 5];
        calls += crate::realtime::allocator_calls(|| processor.process(&mut out[..frames * 2]));
        assert!(out[..frames * 2].iter().all(|sample| sample.is_finite()));
        // Retired plans and their prepared paths are collected on the control side.
        observe_and_retire(&controller);
    }
    rig.remove_effect(upstream);
    rig.remove_effect(downstream);
    controller.set_project(&rig.project, &rig.pool);
    for _ in 0..5 {
        calls += crate::realtime::allocator_calls(|| processor.process(&mut out));
    }
    observe_and_retire(&controller);
    controller.set_project(&rig.project, &rig.pool);
    calls += crate::realtime::allocator_calls(|| processor.process(&mut out));
    assert_eq!(
        calls, 0,
        "causal compensation touched alloc/realloc/free on the callback"
    );
}

#[test]
fn utility_effects_before_and_after_limiters_match_rendering_with_automatic_pdc() {
    for kind in KINDS {
        for utility_first in [true, false] {
            let mut rig = Rig::new();
            let track = rig.track();
            let channel = rig.channel_on(sine(RATE, 173.0, 0.3), track);
            rig.steps(channel, &[0]);
            let utility = configured(kind);
            let limiter = idle_limiter(1.0);
            for params in if utility_first {
                [utility, limiter]
            } else {
                [limiter, utility]
            } {
                rig.effect(track, params);
            }
            let lane = rig.playlist_track();
            let pattern = rig.first_pattern();
            rig.clip(lane, pattern, 0, 1920);
            let (_, controller) = rig.processor(RATE);
            let latency = 48 + utility.latency_samples(RATE as f32);
            assert_eq!(controller.latency_frames() as usize, latency, "{kind:?}");
            let options = RenderOptions {
                mode: PlayMode::Song,
                tail_secs: 0.05,
                block_frames: 97,
                ..Default::default()
            };
            let exported = render(&rig.project, &rig.pool, &options, &mut |_| true);
            let played = rig.play_song(RATE, exported.frames() + latency, 137);
            assert_eq!(
                exported.samples(),
                &played[latency * 2..],
                "{kind:?}/{utility_first}"
            );
            assert!(peak(exported.samples()) > 0.01);
            let options = RenderOptions {
                block_frames: 511,
                ..options
            };
            assert_eq!(
                exported.samples(),
                render(&rig.project, &rig.pool, &options, &mut |_| true).samples()
            );
        }
    }
}

#[test]
fn utility_effects_matrix_common_delay_aligns_plain_tracks_and_keeps_stereo_offset() {
    let mut rig = Rig::new();
    let track = rig.track();
    let plain = rig.track();
    let delayed = rig.channel_on(impulse(RATE), track);
    let direct = rig.channel_on(impulse(RATE), plain);
    rig.channel_mut(delayed).volume = 0.25;
    rig.channel_mut(direct).volume = 0.5;
    rig.steps(delayed, &[1]);
    rig.steps(direct, &[1]);
    rig.effect(track, matrix(2.0, 5.0));
    let (_, controller) = rig.processor(RATE);
    assert_eq!(controller.latency_frames(), 96);
    let audio = rig.play(RATE, 7000, 137);
    let (l, r) = (left(&audio), right(&audio));
    assert_eq!(l[6096], 0.75);
    assert_eq!(r[6096], 0.5);
    assert_eq!(r[6240], 0.25);
    assert_eq!(l.iter().filter(|s| **s != 0.0).count(), 1);
    assert_eq!(r.iter().filter(|s| **s != 0.0).count(), 2);
}

#[test]
fn utility_effects_matrix_latency_edits_keep_opposite_tracks_cancelling() {
    let mut rig = Rig::new();
    let first = rig.track();
    let second = rig.track();
    let tone = sine(RATE, 173.0, 1.0);
    let other = crate::support::inverted(&tone);
    let up = rig.channel_on(tone, first);
    let down = rig.channel_on(other, second);
    rig.steps(up, &[0]);
    rig.steps(down, &[0]);
    let effect = rig.effect(first, matrix(2.0, 2.0));
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    let mut audio = run(&mut processor, 3000, 137);
    for ms in [5.0, 0.0, 50.0, 1.0] {
        rig.effect_mut(effect).params = matrix(ms, ms);
        controller.set_project(&rig.project, &rig.pool);
        assert_eq!(controller.latency_frames(), (ms * 48.0).round() as u32);
        audio.extend(run(&mut processor, 3000, 137));
    }
    assert!(peak(&audio) < 1e-6, "PDC diverged: {}", peak(&audio));
}

#[test]
fn utility_effects_rapid_matrix_latency_edits_keep_dry_wet_and_pdc_in_step() {
    for mix in [0.0, 0.5, 1.0] {
        let mut rig = Rig::new();
        let first = rig.track();
        let second = rig.track();
        let tone = sine(RATE, 1000.0, 1.0);
        let other = crate::support::inverted(&tone);
        let up = rig.channel_on(tone, first);
        let down = rig.channel_on(other, second);
        rig.steps(up, &[0]);
        rig.steps(down, &[0]);
        let effect = rig.effect(first, matrix(2.0, 2.0));
        rig.effect_mut(effect).mix = mix;
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let mut audio = run(&mut processor, 4092, 137);
        for (ms, frames, block) in [
            (0.5, 120, 29),
            (1.0, 37, 1),
            (0.25, 17, 7),
            (1.5, 61, 3),
            (0.75, 1000, 137),
        ] {
            rig.effect_mut(effect).params = matrix(ms, ms);
            controller.set_project(&rig.project, &rig.pool);
            assert_eq!(controller.latency_frames(), (ms * 48.0).round() as u32);
            audio.extend(run(&mut processor, frames, block));
        }
        assert!(
            peak(&audio) < 1e-6,
            "rapid matrix PDC diverged at mix {mix}: {}",
            peak(&audio)
        );
    }
}

#[test]
fn utility_effects_matrix_and_distortion_report_128_samples_even_when_bypassed() {
    for enabled in [false, true] {
        let mut rig = Rig::new();
        let track = rig.track();
        let channel = rig.channel_on(impulse(RATE), track);
        rig.steps(channel, &[1]);
        for params in [
            matrix(2.0, 5.0),
            EffectParams::Distortion(DistortionParams::default()),
        ] {
            let effect = rig.effect(track, params);
            rig.effect_mut(effect).enabled = enabled;
        }
        let (_, controller) = rig.processor(RATE);
        assert_eq!(controller.latency_frames(), 128);
        if !enabled {
            let audio = rig.play(RATE, 7000, 137);
            assert_eq!(left(&audio)[6128], 1.0);
            assert_eq!(right(&audio)[6128], 1.0);
            assert_eq!(audio.iter().filter(|sample| **sample != 0.0).count(), 2);
        }
    }
}

#[test]
fn utility_effects_moves_preserve_processor_state_and_removal_retires_tails() {
    for kind in KINDS {
        let mut rig = Rig::new();
        let track = rig.track();
        let sample = if kind == EffectKind::DcBlock {
            impulse(RATE)
        } else {
            sine(RATE, 173.0, 1.0)
        };
        let channel = rig.channel_on(sample, track);
        rig.steps(channel, &[0]);
        let effect = rig.effect(track, configured(kind));
        let reference = rig.play(RATE, 12000, 128);
        let (mut processor, controller) = rig.processor(RATE);
        controller.play();
        let mut moved = run(&mut processor, 4000, 128);
        let slot = rig.remove_effect(effect);
        rig.track_mut(TrackId::MASTER).effects.push(slot);
        controller.set_project(&rig.project, &rig.pool);
        moved.extend(run(&mut processor, 8000, 128));
        // An effect moved to another track fades in for 5 ms. After that its
        // history is carried, including DC memory and both FIR delay lines.
        let settled = 4000 + 240 + configured(kind).latency_samples(RATE as f32);
        assert_eq!(
            &moved[settled * 2..],
            &reference[settled * 2..],
            "{kind:?} moved state"
        );
        rig.remove_effect(effect);
        controller.set_project(&rig.project, &rig.pool);
        let out = run(&mut processor, 4000, 128);
        assert!(out.iter().all(|x| x.is_finite()));
        controller.set_project(&rig.project, &rig.pool);
        let out = run(&mut processor, 1000, 128);
        assert_eq!(controller.latency_frames(), 0);
        if kind == EffectKind::DcBlock {
            assert_eq!(peak(&out), 0.0);
        } else {
            assert!(rms(&left(&out)) > 0.2);
        }
    }
}

#[test]
fn utility_effects_live_automation_changes_each_real_processor_and_exports_the_same_audio() {
    // Choose a non-latency setting for this test; common delay changes have
    // their own exact cancellation test above.
    for (kind, param, value) in [
        (EffectKind::Balance, 0, 0.0),
        (EffectKind::DcBlock, 0, 1.0),
        (EffectKind::ChannelMute, 0, 1.0),
        (EffectKind::Polarity, 0, 1.0),
        (EffectKind::StereoMatrix, 1, 0.5),
        (EffectKind::SoftClipper, 0, 0.0),
        (EffectKind::Distortion, 0, 1.0),
    ] {
        let mut rig = Rig::new();
        let track = rig.track();
        let channel = rig.channel_on(level(RATE, 0.5, 0.5), track);
        rig.steps(channel, &[0]);
        let effect = rig.effect(track, kind.default_params());
        let lane = rig.playlist_track();
        let pattern = rig.first_pattern();
        rig.clip(lane, pattern, 0, 960);
        let dry = rig.play_song(RATE, 24000, 137);
        let lane = rig.playlist_track();
        let automation = rig.automation(
            AutomationTarget::EffectParam {
                track,
                effect,
                param,
            },
            &[(0, value)],
        );
        rig.automation_clip(lane, automation, 0, 960);
        let options = RenderOptions {
            mode: PlayMode::Song,
            block_frames: 101,
            ..Default::default()
        };
        let exported = render(&rig.project, &rig.pool, &options, &mut |_| true);
        let (_, controller) = rig.processor(RATE);
        let latency = controller.latency_frames() as usize;
        let played = rig.play_song(RATE, exported.frames() + latency, 137);
        assert_eq!(
            exported.samples(),
            &played[latency * 2..],
            "{kind:?} automation parity"
        );
        // DC difference is most visible during the step decay, the others
        // after the control has settled. The effect must change actual audio.
        let range = if kind == EffectKind::DcBlock {
            250..1000
        } else {
            1000..8000
        };
        assert!(
            range
                .map(|i| (played[(i + latency) * 2] - dry[(i + latency) * 2]).abs())
                .fold(0.0_f32, f32::max)
                > 1e-3,
            "{kind:?} cosmetic automation"
        );
    }
}

#[test]
fn utility_effects_finite_tails_survive_auto_tail_rendering() {
    for params in [
        matrix(50.0, 10.0),
        EffectParams::DcBlock(DcBlockParams { cutoff_hz: 40.0 }),
        EffectParams::Distortion(DistortionParams::default()),
    ] {
        let mut rig = Rig::new();
        let track = rig.track();
        let lane = rig.playlist_track();
        rig.audio_clip(lane, level(RATE, 0.5, 0.002), track, 0, 4);
        rig.effect(track, params);
        let options = RenderOptions {
            mode: PlayMode::Song,
            tail_secs: 1.0,
            auto_tail: true,
            block_frames: 113,
            ..Default::default()
        };
        let exported = render(&rig.project, &rig.pool, &options, &mut |_| true);
        assert!(exported.frames() < 48025);
        assert!(exported.samples().iter().all(|x| x.is_finite()));
        assert!(peak(exported.samples()) > 1e-4, "{:?}", params.kind());
    }
}
