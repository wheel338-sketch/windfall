//! Mounted slots, including topology edits, bypass and realtime allocation.
//! Written, not executed. Direct response measurements live next to the DSP.
use windfall_dsp::{AnyEffect, EffectKind, EffectParams, EffectSlot};

const KINDS: [EffectKind; 6] = [
    EffectKind::Room,
    EffectKind::LushSpace,
    EffectKind::VintageChorus,
    EffectKind::VintagePhaser,
    EffectKind::HyperChorus,
    EffectKind::StackedFlanger,
];

fn slot(params: &EffectParams) -> EffectSlot {
    let mut slot = EffectSlot::new(AnyEffect::new(params));
    slot.prepare(8000.0, 127);
    slot.set_params(params);
    slot
}

#[test]
fn e3_slots_allocate_nothing_through_topology_tempo_reset_bypass_and_tail_expiry() {
    for kind in KINDS {
        let mut params = kind.default_params();
        for (i, info) in kind.descriptors().iter().enumerate() {
            params.set(i, info.max);
        }
        let mut slot = slot(&params);
        let (mut left, mut right) = ([0.2; 127], [-0.1; 127]);
        let calls = super::realtime::allocator_calls(|| {
            for n in 0..1000 {
                slot.set_params(&params);
                slot.set_tempo(if n % 2 == 0 { 73.0 } else { 173.0 });
                slot.set_enabled(n % 19 < 11);
                slot.set_mix((n % 5) as f32 / 4.0);
                if n % 37 == 0 {
                    slot.reset();
                }
                slot.process(&mut left, &mut right);
                left.fill(0.0);
                right.fill(0.0);
                let _ = (
                    slot.latency_samples(),
                    slot.tail_samples(),
                    slot.gap_samples(),
                );
            }
            slot.set_enabled(true);
            slot.reset();
            // Enough silence for the longest reverb lifetime and its clear.
            for _ in 0..1200 {
                slot.process(&mut left, &mut right);
            }
        });
        assert_eq!(calls, 0, "{kind:?}");
    }
}

#[test]
fn e3_bypass_wake_and_parameter_edits_are_partition_invariant_and_return_exact_dry() {
    for kind in KINDS {
        let params = kind.default_params();
        let mut whole = slot(&params);
        let mut split = slot(&params);
        let (mut left, mut right) = (vec![0.1; 8000], vec![-0.1; 8000]);
        let (mut sl, mut sr) = (left.clone(), right.clone());
        for (start, end, enabled) in [
            (0, 1001, true),
            (1001, 3003, false),
            (3003, 5005, true),
            (5005, 8000, false),
        ] {
            whole.set_enabled(enabled);
            split.set_enabled(enabled);
            let mut changed = params;
            if start == 3003 {
                for (i, info) in kind.descriptors().iter().enumerate() {
                    changed.set(i, info.max);
                }
            }
            whole.set_params(&changed);
            split.set_params(&changed);
            whole.process(&mut left[start..end], &mut right[start..end]);
            let mut at = start;
            while at < end {
                let next = (at + 7).min(end);
                split.process(&mut sl[at..next], &mut sr[at..next]);
                at = next;
            }
        }
        assert_eq!(left, sl, "{kind:?}");
        assert_eq!(right, sr);
        assert!(left[7000..].iter().all(|x| *x == 0.1));
        assert!(right[7000..].iter().all(|x| *x == -0.1));
        assert!(left.iter().chain(&right).all(|x| x.is_finite()));
        // Silence on dormant wake must not replay the previous constant input.
        whole.set_enabled(true);
        left.fill(0.0);
        right.fill(0.0);
        whole.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|x| *x == 0.0));
    }
}
