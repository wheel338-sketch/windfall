//! Owned prototype and production graph/dispatcher acceptance.
use super::realtime::allocator_calls;
use std::time::{Duration, Instant};
use windfall_dsp::{TrackParams, TrackProcessor};
use windfall_engine::track_workers::{
    GraphWaves, MAX_WORKERS, TRACK_BLOCK_FRAMES, TrackJob, TrackWorkers,
};

fn job(track: usize) -> TrackJob {
    let params = TrackParams {
        separation: 0.23,
        invert_right: track.is_multiple_of(2),
        ..Default::default()
    };
    let mut job = TrackJob::new(
        7,
        track,
        TrackProcessor::new(48_000, TRACK_BLOCK_FRAMES, params),
    );
    let left: [f32; TRACK_BLOCK_FRAMES] =
        std::array::from_fn(|i| ((i * 13 + track * 17) as f32 * 0.017).sin() * 0.2);
    let right: [f32; TRACK_BLOCK_FRAMES] =
        std::array::from_fn(|i| ((i * 7 + track * 19) as f32 * 0.021).cos() * 0.1);
    assert!(job.load(&left, &right));
    job
}

fn harvest(pool: &mut TrackWorkers, lane: usize) -> TrackJob {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(job) = pool.try_take(lane) {
            return job;
        }
        assert!(
            Instant::now() < deadline,
            "worker failed to return ownership"
        );
        std::thread::yield_now(); // Test only, never in the audio implementation.
    }
}

#[test]
fn track_workers_match_serial_bits_across_blocks_and_reversed_completion_order() {
    let mut pool = TrackWorkers::new(3).unwrap();
    let mut serial = [job(0), job(1)];
    let mut parallel = [Some(job(0)), Some(job(1))];
    for block in 0..32 {
        for (lane, reference) in serial.iter_mut().enumerate() {
            reference.process();
            assert!(
                pool.try_submit(lane, parallel[lane].take().unwrap())
                    .is_ok()
            );
        }
        // Arrival/harvesting order must not become floating-point sum order.
        parallel[1] = Some(harvest(&mut pool, 1));
        parallel[0] = Some(harvest(&mut pool, 0));
        let mut expected = [0.0_f32; TRACK_BLOCK_FRAMES];
        let mut actual = [0.0_f32; TRACK_BLOCK_FRAMES];
        for (lane, reference) in serial.iter().enumerate() {
            let found = parallel[lane].as_ref().unwrap();
            assert_eq!((found.epoch(), found.track()), (7, lane));
            for (i, (expected, actual)) in expected.iter_mut().zip(&mut actual).enumerate() {
                assert_eq!(
                    found.left()[i].to_bits(),
                    reference.left()[i].to_bits(),
                    "block {block}, track {lane}, frame {i}"
                );
                assert_eq!(found.right()[i].to_bits(), reference.right()[i].to_bits());
                *expected += reference.left()[i];
                *actual += found.left()[i];
            }
        }
        assert_eq!(expected.map(f32::to_bits), actual.map(f32::to_bits));
    }
}

#[test]
fn bounded_handoff_exhaustion_and_deadline_miss_return_or_retain_the_owner_without_allocator_calls()
{
    let mut pool = TrackWorkers::new(2).unwrap();
    let mut first = Some(job(0));
    let mut refused = Some(job(1));
    let mut completed: [Option<TrackJob>; MAX_WORKERS] = std::array::from_fn(|_| None);
    let calls = allocator_calls(|| {
        assert!(pool.try_submit(0, first.take().unwrap()).is_ok());
        refused = pool.try_submit(0, refused.take().unwrap()).err();
        // A zero budget is a deterministic fault injection, not a timing guess.
        assert_eq!(pool.poll_budget(0, &mut completed), 1);
        assert!(pool.is_fallback());
        refused.as_mut().unwrap().process();
        assert!(!pool.resume()); // Cannot retire or reconfigure an in-flight owner.
    });
    assert_eq!(calls, 0);
    let returned = harvest(&mut pool, 0);
    assert_eq!(returned.track(), 0);
    assert_eq!(pool.pending(), 0);
    assert!(pool.resume());
    assert_eq!(refused.unwrap().track(), 1);
}

#[test]
fn one_thread_and_invalid_lane_use_serial_without_spawning_or_losing_track_state() {
    let mut pool = TrackWorkers::new(1).unwrap();
    assert_eq!(pool.worker_count(), 0);
    let mut owned = Some(job(3));
    assert_eq!(
        allocator_calls(|| {
            owned = pool.try_submit(0, owned.take().unwrap()).err();
            owned.as_mut().unwrap().process();
        }),
        0
    );
    assert_eq!(owned.unwrap().track(), 3);
    assert!(TrackWorkers::new(0).is_err());
    assert!(TrackWorkers::new(MAX_WORKERS + 2).is_err());
}

#[test]
fn graph_waves_preserve_serial_sum_order_and_include_detector_dependencies() {
    // 2 and 3 feed 1; 1 feeds master; 4 detector-feeds 3.
    let order = [2, 4, 3, 1, 0];
    let waves = GraphWaves::compile(&order, 5, &[(2, 1), (4, 3), (3, 1), (1, 0)], &[]).unwrap();
    assert_eq!(waves.ranges(), &[0..2, 2..3, 3..4, 4..5]);
    let serial_current =
        GraphWaves::compile(&order, 5, &[(2, 1), (4, 3), (3, 1), (1, 0)], &[4]).unwrap();
    assert_eq!(serial_current.ranges(), &[0..1, 1..2, 2..3, 3..4, 4..5]);
    assert!(GraphWaves::compile(&order, 5, &[(0, 2)], &[]).is_err());
    assert!(GraphWaves::compile(&[0, 0], 2, &[], &[]).is_err());
}

fn routed_rig() -> crate::support::Rig {
    use crate::support::{Rig, idle_limiter, plain_synth, sine};
    use windfall_dsp::{CompressorParams, EqParams, ReverbParams};
    use windfall_project::{AutomationTarget, EffectParams, Send};
    let mut rig = Rig::new();
    let bus = rig.track();
    let wet = rig.track();
    rig.track_mut(wet).output = Some(bus);
    rig.effect(wet, EffectParams::Reverb(ReverbParams::default()));
    rig.effect(bus, idle_limiter(3.0));
    let mut sources = Vec::new();
    for index in 0usize..16 {
        let track = rig.track();
        sources.push(track);
        rig.track_mut(track).output = Some(bus);
        rig.track_mut(track).processing.separation = 0.21;
        rig.track_mut(track).sends.push(Send {
            target: wet,
            gain: 0.03 + index as f32 * 0.002,
        });
        let channel = if index.is_multiple_of(2) {
            rig.synth_on(plain_synth(), track)
        } else {
            rig.channel_on(sine(48_000, 110.0 + index as f64 * 37.0, 2.0), track)
        };
        rig.channel_mut(channel).volume = 0.07;
        rig.note(channel, 0, 960).key = 48 + index as u8;
        rig.effect(track, EffectParams::Eq(EqParams::default()));
        if index.is_multiple_of(3) {
            rig.effect(track, idle_limiter(1.0 + index as f32 * 0.1));
        }
    }
    rig.track_mut(sources[0]).sidechains.push(Send {
        target: sources[1],
        gain: 0.8,
    });
    rig.effect(
        sources[1],
        EffectParams::Compressor(CompressorParams {
            sidechain: true,
            ..Default::default()
        }),
    );
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 960);
    let automation = rig.automation(
        AutomationTarget::TrackVolume { track: sources[2] },
        &[(0, 0.3), (400, 0.8), (960, 0.4)],
    );
    rig.automation_clip(lane, automation, 0, 960);
    rig
}

fn production_audio(rig: &crate::support::Rig, threads: usize, random: bool) -> Vec<f32> {
    let (mut processor, controller) = rig.song_processor(48_000);
    controller.set_audio_threads(Some(threads)).unwrap();
    processor.process(&mut []); // adopt prepared seats without advancing DSP
    let latency = controller.latency_frames() as usize;
    assert!(latency > 0, "fixture must exercise compensation");
    controller.play();
    let mut audio = vec![0.0; (24_000 + latency) * 2];
    let mut position = 0;
    let mut random_state = 0x8546_7749u32;
    while position < audio.len() {
        random_state = random_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let frames = if random {
            1 + random_state as usize % 509
        } else {
            256
        };
        let end = (position + frames * 2).min(audio.len());
        assert_eq!(
            allocator_calls(|| processor.process(&mut audio[position..end])),
            0
        );
        position = end;
    }
    controller.frame();
    audio[latency * 2..].to_vec()
}

#[test]
fn production_tracks_sends_sidechains_buses_pdc_and_automation_match_serial_bits_for_all_thread_counts_and_partitions()
 {
    use windfall_engine::{RenderOptions, render};
    use windfall_ipc::PlayMode;
    let rig = routed_rig();
    let expected = production_audio(&rig, 1, false);
    assert!(expected.iter().any(|sample| sample.abs() > 0.001));
    let bits = |audio: &[f32]| {
        audio
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    };
    for threads in [1, 2, 4, 8] {
        for random in [false, true] {
            assert_eq!(
                bits(&production_audio(&rig, threads, random)),
                bits(&expected),
                "threads={threads}, random partitions={random}"
            );
        }
        let rendered = render(
            &rig.project,
            &rig.pool,
            &RenderOptions {
                audio_threads: threads,
                mode: PlayMode::Song,
                block_frames: 191,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert_eq!(
            bits(rendered.samples()),
            bits(&expected),
            "offline threads={threads}"
        );
    }
}

#[test]
fn live_thread_count_changes_and_plan_edits_preserve_dsp_bits_and_allocate_nothing_on_callback() {
    let mut rig = routed_rig();
    let (mut serial, serial_control) = rig.song_processor(48_000);
    let (mut parallel, parallel_control) = rig.song_processor(48_000);
    serial_control.play();
    parallel_control.play();
    let mut reference = [0.0; 512];
    let mut actual = [0.0; 512];
    let bus = rig.project.mixer.tracks[1].id;
    for round in 0..24 {
        parallel_control
            .set_audio_threads(Some([1, 2, 4, 8][round % 4]))
            .unwrap();
        rig.track_mut(bus).pan = (round as f32 * 0.17).sin() * 0.6;
        serial_control.set_project(&rig.project, &rig.pool);
        parallel_control.set_project(&rig.project, &rig.pool);
        assert_eq!(allocator_calls(|| serial.process(&mut reference)), 0);
        assert_eq!(allocator_calls(|| parallel.process(&mut actual)), 0);
        assert_eq!(
            actual.map(f32::to_bits),
            reference.map(f32::to_bits),
            "live swap {round}"
        );
        let mut retired = windfall_engine::ProjectRetirement::default();
        parallel_control.take_retired(&mut retired);
        drop(retired);
        let mut retired = windfall_engine::ProjectRetirement::default();
        serial_control.take_retired(&mut retired);
        drop(retired);
    }
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert!(parallel_control.set_audio_threads(Some(0)).is_err());
    assert!(parallel_control.set_audio_threads(Some(9)).is_err());
    parallel_control.set_audio_threads(None).unwrap();
    assert!((1..=8).contains(&parallel_control.audio_threads()));
}

#[test]
fn release_many_heavy_tracks_reports_scaling_above_a_modest_multicore_floor() {
    use crate::support::{Rig, plain_synth};
    use windfall_dsp::{EffectParams, ReverbParams};
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    if cfg!(debug_assertions) || cores < 2 {
        // The release/multicore gate is explicit; debug still checks its admission.
        assert!(cores >= 1);
        eprintln!(
            "scaling measurement requires release mode and at least two available cores (cores={cores})"
        );
        return;
    }
    let mut rig = Rig::new();
    for _ in 0..32 {
        let track = rig.track();
        let channel = rig.synth_on(plain_synth(), track);
        rig.channel_mut(channel).volume = 0.01;
        for key in 48..56 {
            rig.note(channel, 0, 7680).key = key;
        }
        for _ in 0..8 {
            rig.effect(track, EffectParams::Reverb(ReverbParams::default()));
        }
    }
    let measure = |threads: usize| {
        let (mut processor, control) = rig.processor(48_000);
        control.set_audio_threads(Some(threads)).unwrap();
        control.play();
        let mut block = [0.0; 512];
        for _ in 0..16 {
            processor.process(&mut block);
        }
        let started = Instant::now();
        let mut energy = 0.0f64;
        for _ in 0..128 {
            processor.process(&mut block);
            energy += block.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        }
        let elapsed = started.elapsed();
        assert!(energy.is_finite() && energy > 0.0);
        elapsed
    };
    let threads = cores.min(4);
    // Paired trials in alternating order limit thermal/order bias. Best pair
    // tolerates transient unrelated machine load without weakening the floor.
    let mut best = 0.0f64;
    for trial in 0..5 {
        let (serial, parallel) = if trial % 2 == 0 {
            (measure(1), measure(threads))
        } else {
            let parallel = measure(threads);
            (measure(1), parallel)
        };
        let speedup = serial.as_secs_f64() / parallel.as_secs_f64();
        eprintln!(
            "heavy DSP trial={trial}, cores={cores}, threads={threads}, serial={serial:?}, parallel={parallel:?}, speedup={speedup:.3}x"
        );
        best = best.max(speedup);
    }
    assert!(
        best > 1.02,
        "heavy production graph did not exceed the 1.02x multicore floor: best={best:.3}x"
    );
}
