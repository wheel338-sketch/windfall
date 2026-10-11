//! Test-first contract for owned track work. This does not assert that the
//! production mixer currently uses the pool.
use super::realtime::allocator_calls;
use std::time::{Duration, Instant};
use windfall_dsp::{TrackParams, TrackProcessor};
use windfall_engine::track_workers::{
    GraphWaves, MAX_WORKERS, TRACK_BLOCK_FRAMES, TrackJob, TrackWorkers,
};

fn job(track: usize) -> TrackJob {
    let params = TrackParams {
        separation: 0.23,
        invert_right: track % 2 == 0,
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
        for lane in 0..2 {
            serial[lane].process();
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
        for lane in 0..2 {
            let found = parallel[lane].as_ref().unwrap();
            assert_eq!((found.epoch(), found.track()), (7, lane));
            for i in 0..TRACK_BLOCK_FRAMES {
                assert_eq!(
                    found.left()[i].to_bits(),
                    serial[lane].left()[i].to_bits(),
                    "block {block}, track {lane}, frame {i}"
                );
                assert_eq!(
                    found.right()[i].to_bits(),
                    serial[lane].right()[i].to_bits()
                );
                expected[i] += serial[lane].left()[i];
                actual[i] += found.left()[i];
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
