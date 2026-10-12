//! Public registry routing, distinct from the acoustic reference suite.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use serde_json::json;
use windfall_dsp::{
    AnyEffect, BassShelf, BassShelfParams, Effect, EffectKind, EffectParams, EffectSlot,
    FastLowpass, FastLowpassParams, ParamKind, SelectableFilter, SelectableFilterMode as Mode,
    SelectableFilterParams,
};

const KINDS: [EffectKind; 3] = [
    EffectKind::FastLowpass,
    EffectKind::SelectableFilter,
    EffectKind::BassShelf,
];

// Same TLS allocator guard as the existing DSP realtime tests. Construction,
// prepare and destruction remain outside the callback guard.
thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;
fn count() {
    if WATCHING.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}

// SAFETY: calls forward unchanged to System; bookkeeping uses plain TLS cells.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: caller upholds the allocator contract.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: caller upholds the allocator contract.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        // SAFETY: caller upholds the allocator contract.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: caller upholds the allocator contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocator_calls(work: impl FnOnce()) -> usize {
    struct Stop;
    impl Drop for Stop {
        fn drop(&mut self) {
            WATCHING.set(false);
        }
    }
    CALLS.set(0);
    WATCHING.set(true);
    let stop = Stop;
    work();
    drop(stop);
    CALLS.get()
}

fn signal(seed: usize, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| ((i * 37 + seed * 101) % 257) as f32 / 257.0 * 0.2 - 0.1)
        .collect()
}

#[test]
fn kinds_are_appended_and_have_distinct_tags_names_defaults_and_controls() {
    assert_eq!(EffectKind::ALL.len(), 67);
    // Later effect families append without displacing this family's slots.
    let mut tags = std::collections::BTreeSet::new();
    for (index, kind) in EffectKind::ALL.iter().enumerate() {
        assert_eq!(*kind as usize, index);
        assert!(tags.insert(serde_json::to_string(kind).unwrap()));
        let mut controls = std::collections::BTreeSet::new();
        for info in kind.descriptors() {
            assert!(
                controls.insert(info.id),
                "{} has duplicate control {}",
                kind.name(),
                info.id
            );
        }
    }
    assert_eq!(EffectKind::ALL[12..15], KINDS);
    // Preserve the previous registration order and discriminants used by
    // test/random settings and existing enum consumers.
    let old = [
        "eq",
        "compressor",
        "limiter",
        "reverb",
        "delay",
        "balance",
        "dcBlock",
        "channelMute",
        "polarity",
        "stereoMatrix",
        "softClipper",
        "distortion",
    ];
    for (i, kind) in EffectKind::ALL[..12].iter().enumerate() {
        assert_eq!(*kind as usize, i);
        assert_eq!(serde_json::to_value(kind).unwrap(), old[i]);
    }
    for (kind, tag, label, ids) in [
        (
            EffectKind::FastLowpass,
            "fastLowpass",
            "Fast lowpass",
            &["cutoffHz", "q"][..],
        ),
        (
            EffectKind::SelectableFilter,
            "selectableFilter",
            "Selectable filter",
            &["mode", "frequencyHz", "q", "gainDb"][..],
        ),
        (
            EffectKind::BassShelf,
            "bassShelf",
            "Bass shelf",
            &["frequencyHz", "gainDb"][..],
        ),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), tag);
        assert_eq!(kind.name(), label);
        assert_eq!(
            kind.descriptors()
                .iter()
                .map(|info| info.id)
                .collect::<Vec<_>>(),
            ids
        );
        let defaults = kind.default_params();
        assert_eq!(defaults.kind(), kind);
        assert_eq!(defaults.sanitized(), defaults);
        let encoded = serde_json::to_value(defaults).unwrap();
        assert_eq!(encoded["type"], tag);
        assert_eq!(
            serde_json::from_value::<EffectParams>(encoded.clone()).unwrap(),
            defaults
        );
        assert_eq!(
            serde_json::from_value::<EffectParams>(json!({ "type": tag })).unwrap(),
            defaults
        );
        for (index, info) in kind.descriptors().iter().enumerate() {
            assert_eq!(defaults.get(index), Some(info.default));
            let expected = if info.kind == ParamKind::Choice {
                json!(info.choices[info.default as usize].value)
            } else {
                json!(info.default)
            };
            assert_eq!(encoded[info.id], expected);
            let mut changed = defaults;
            assert!(changed.set(index, info.max));
            assert_eq!(changed.get(index), Some(info.max));
            assert_eq!(changed.sanitized(), changed);
        }
        let mut untouched = defaults;
        assert_eq!(untouched.get(ids.len()), None);
        assert!(!untouched.set(ids.len(), 0.0));
        assert_eq!(untouched, defaults);
        // This is exactly the descriptor shape emitted by the example,
        // without writing the downstream generated JSON artifact.
        let descriptor =
            json!({ "name": kind.name(), "params": kind.descriptors(), "defaults": defaults });
        assert_eq!(descriptor["defaults"]["type"], tag);
        assert_eq!(descriptor["params"].as_array().unwrap().len(), ids.len());
        let effect = AnyEffect::new(&defaults);
        assert_eq!(effect.kind(), kind);
        assert!(matches!(
            (kind, effect),
            (EffectKind::FastLowpass, AnyEffect::FastLowpass(_))
                | (EffectKind::SelectableFilter, AnyEffect::SelectableFilter(_))
                | (EffectKind::BassShelf, AnyEffect::BassShelf(_))
        ));
    }
    let choices = &EffectKind::SelectableFilter.descriptors()[0].choices;
    assert_eq!(
        choices
            .iter()
            .map(|choice| choice.value)
            .collect::<Vec<_>>(),
        [
            "lowpass",
            "highpass",
            "bandpass",
            "notch",
            "lowShelf",
            "peak",
            "highShelf"
        ]
    );
}

#[test]
fn malformed_serialized_settings_reject_bad_types_and_finite_values_sanitize() {
    for value in [
        json!({ "type": "fastLowpass", "cutoffHz": "NaN" }),
        json!({ "type": "fastLowpass", "q": null }),
        json!({ "type": "selectableFilter", "mode": "unknown" }),
        json!({ "type": "selectableFilter", "mode": 3 }),
        json!({ "type": "bassShelf", "gainDb": "Infinity" }),
        json!({ "type": "unknown" }),
    ] {
        assert!(serde_json::from_value::<EffectParams>(value).is_err());
    }
    for (value, expected) in [
        (
            json!({ "type": "fastLowpass", "cutoffHz": -10, "q": 1000 }),
            EffectParams::FastLowpass(FastLowpassParams {
                cutoff_hz: 20.0,
                q: 10.0,
            }),
        ),
        (
            json!({ "type": "selectableFilter", "mode": "highShelf", "frequencyHz": 1e8, "q": -10, "gainDb": -100 }),
            EffectParams::SelectableFilter(SelectableFilterParams {
                mode: Mode::HighShelf,
                frequency_hz: 20_000.0,
                q: 0.5,
                gain_db: -18.0,
            }),
        ),
        (
            json!({ "type": "bassShelf", "frequencyHz": -100, "gainDb": 100 }),
            EffectParams::BassShelf(BassShelfParams {
                frequency_hz: 40.0,
                gain_db: 18.0,
            }),
        ),
    ] {
        let imported = serde_json::from_value::<EffectParams>(value).unwrap();
        assert_eq!(imported.sanitized(), expected);
        // Construction and parameter updates sanitize just as direct DSP
        // does: bad finite input must never reach coefficient generation.
        let mut actual = AnyEffect::new(&imported);
        let mut clean = AnyEffect::new(&expected);
        actual.prepare(48_000.0, 127);
        clean.prepare(48_000.0, 127);
        let (mut al, mut ar) = (signal(11, 4096), signal(13, 4096));
        let (mut cl, mut cr) = (al.clone(), ar.clone());
        actual.process(&mut al, &mut ar);
        clean.process(&mut cl, &mut cr);
        assert_eq!(al, cl);
        assert_eq!(ar, cr);
    }
    for dirty in [
        EffectParams::FastLowpass(FastLowpassParams {
            cutoff_hz: f32::NAN,
            q: f32::INFINITY,
        }),
        EffectParams::SelectableFilter(SelectableFilterParams {
            frequency_hz: f32::NAN,
            q: f32::NEG_INFINITY,
            gain_db: f32::INFINITY,
            ..Default::default()
        }),
        EffectParams::BassShelf(BassShelfParams {
            frequency_hz: f32::INFINITY,
            gain_db: f32::NAN,
        }),
    ] {
        assert_eq!(dirty.sanitized(), dirty.kind().default_params());
    }
}

fn delegation<E: Effect + Default>(
    kind: EffectKind,
    wrap: fn(E::Params) -> EffectParams,
    settings: &[E::Params],
) {
    for rate in [1.0_f32, 8_000.0, 48_000.0, 384_000.0] {
        let mut direct = E::default();
        direct.set_params(&settings[0]);
        direct.prepare(rate, 127);
        let mut effect = AnyEffect::new(&wrap(settings[0]));
        effect.prepare(rate, 127);
        assert_eq!(effect.kind(), kind);
        assert_eq!(effect.gain_reduction().map(|meter| meter.take_db()), None);
        assert_eq!(kind.max_latency_samples(rate), 0);
        assert_eq!(wrap(settings[0]).latency_samples(rate), 0);
        assert_eq!(effect.max_latency_samples(rate), 0);
        assert_eq!(effect.latency_samples(), direct.latency_samples());
        assert_eq!(effect.warm_up_samples(), direct.warm_up_samples());
        assert_eq!(
            effect.delay_readiness_samples(),
            direct.delay_readiness_samples()
        );
        assert_eq!(effect.latency_transition_samples_remaining(), 0);
        assert_eq!(effect.tail_samples(), direct.tail_samples());
        assert_eq!(effect.gap_samples(), direct.gap_samples());
        let mut noop = AnyEffect::new(&wrap(settings[0]));
        noop.prepare(rate, 127);
        for (step, params) in settings.iter().cycle().take(30).enumerate() {
            direct.set_params(params);
            assert!(effect.set_params(&wrap(*params)));
            assert!(noop.set_params(&wrap(*params)));
            assert!(!effect.set_params(&EffectKind::Eq.default_params()));
            effect.set_tempo(f32::NAN);
            noop.set_tempo(133.0);
            if step % 7 == 0 {
                direct.reset();
                effect.reset();
                noop.reset();
            }
            let (mut dl, mut dr) = (signal(step, 511), signal(step + 31, 511));
            let (mut el, mut er) = (dl.clone(), dr.clone());
            let (mut nl, mut nr) = (dl.clone(), dr.clone());
            direct.process(&mut dl, &mut dr);
            effect.process(&mut el, &mut er);
            for (l, r) in nl.chunks_mut(7).zip(nr.chunks_mut(7)) {
                assert!(noop.set_params(&wrap(*params)));
                noop.process(&mut [], &mut []);
                noop.process(l, r);
            }
            assert_eq!(el, dl, "{kind:?} left dispatch rate {rate} step {step}");
            assert_eq!(er, dr, "{kind:?} right dispatch rate {rate} step {step}");
            assert_eq!(nl, el);
            assert_eq!(nr, er);
        }
        effect.reset();
        let (mut l, mut r) = ([0.0; 1024], [0.0; 1024]);
        effect.process(&mut l, &mut r);
        assert_eq!(l, [0.0; 1024]);
        assert_eq!(r, [0.0; 1024]);
    }
}

#[test]
fn any_effect_dispatches_processing_controls_reset_and_metadata_to_each_processor() {
    delegation::<FastLowpass>(
        EffectKind::FastLowpass,
        EffectParams::FastLowpass,
        &[
            FastLowpassParams {
                cutoff_hz: 20.0,
                q: 10.0,
            },
            FastLowpassParams::default(),
            FastLowpassParams {
                cutoff_hz: 1_000.0,
                q: 0.5,
            },
        ],
    );
    let settings: Vec<_> = Mode::ALL
        .into_iter()
        .enumerate()
        .map(|(i, mode)| SelectableFilterParams {
            mode,
            frequency_hz: if i % 2 == 0 { 20.0 } else { 20_000.0 },
            q: if i % 2 == 0 { 10.0 } else { 0.5 },
            gain_db: if i % 2 == 0 { 18.0 } else { -18.0 },
        })
        .collect();
    delegation::<SelectableFilter>(
        EffectKind::SelectableFilter,
        EffectParams::SelectableFilter,
        &settings,
    );
    delegation::<BassShelf>(
        EffectKind::BassShelf,
        EffectParams::BassShelf,
        &[
            BassShelfParams::default(),
            BassShelfParams {
                frequency_hz: 40.0,
                gain_db: 18.0,
            },
            BassShelfParams {
                frequency_hz: 1_000.0,
                gain_db: 6.0,
            },
        ],
    );
}

fn active(kind: EffectKind) -> EffectParams {
    match kind {
        EffectKind::FastLowpass => EffectParams::FastLowpass(FastLowpassParams {
            cutoff_hz: 1_000.0,
            q: 2.0,
        }),
        EffectKind::SelectableFilter => EffectParams::SelectableFilter(SelectableFilterParams {
            mode: Mode::Highpass,
            frequency_hz: 1_000.0,
            q: 2.0,
            gain_db: 12.0,
        }),
        EffectKind::BassShelf => EffectParams::BassShelf(BassShelfParams {
            frequency_hz: 150.0,
            gain_db: 18.0,
        }),
        _ => panic!("a filter-family kind is required"),
    }
}

#[test]
fn effect_slot_runs_real_processors_at_full_and_partial_mix() {
    for kind in KINDS {
        let params = active(kind);
        for mix in [0.0_f32, 0.37, 1.0] {
            let mut direct = AnyEffect::new(&params);
            direct.prepare(48_000.0, 127);
            let mut slot = EffectSlot::new(AnyEffect::new(&params));
            slot.prepare(48_000.0, 127);
            slot.set_mix(mix);
            let (input_l, input_r) = (signal(19, 4096), signal(43, 4096));
            let (mut wet_l, mut wet_r) = (input_l.clone(), input_r.clone());
            let (mut l, mut r) = (input_l.clone(), input_r.clone());
            direct.process(&mut wet_l, &mut wet_r);
            slot.process(&mut l, &mut r);
            assert_eq!(slot.effect().kind(), kind);
            assert_eq!(slot.latency_samples(), 0);
            if mix > 0.0 {
                assert_eq!(slot.tail_samples(), direct.tail_samples());
                assert_eq!(slot.gap_samples(), direct.gap_samples());
                assert_ne!(l, input_l, "{kind:?} must have an actual transfer");
            } else {
                assert_eq!(slot.tail_samples(), 0);
                assert_eq!(slot.gap_samples(), 0);
            }
            for i in 0..l.len() {
                let expected_l = input_l[i] + (wet_l[i] - input_l[i]) * mix;
                let expected_r = input_r[i] + (wet_r[i] - input_r[i]) * mix;
                assert!((l[i] - expected_l).abs() < 1.0e-7);
                assert!((r[i] - expected_r).abs() < 1.0e-7);
            }
        }
    }
}

#[test]
fn slots_keep_existing_bypass_fade_and_clear_history_on_dormant_wake() {
    for kind in KINDS {
        let params = active(kind);
        let mut slot = EffectSlot::new(AnyEffect::new(&params));
        slot.prepare(48_000.0, 128);
        let (mut l, mut r) = ([0.1; 2048], [-0.1; 2048]);
        slot.process(&mut l, &mut r);
        slot.set_enabled(false);
        let (mut l, mut r) = ([0.0; 480], [0.0; 480]);
        assert!(slot.tail_samples() > 0 && slot.gap_samples() > 0);
        slot.process(&mut l, &mut r);
        assert_eq!(l[479], 0.0);
        assert_eq!(r[479], 0.0);
        assert_eq!(slot.tail_samples(), 0);
        assert_eq!(slot.gap_samples(), 0);
        slot.set_enabled(true);
        let (mut l, mut r) = ([0.0; 512], [0.0; 512]);
        slot.process(&mut l, &mut r);
        assert_eq!(l, [0.0; 512]);
        assert_eq!(r, [0.0; 512]);
        assert_eq!(slot.latency_samples(), 0);
        assert_eq!(slot.effect().warm_up_samples(), 0);
        assert!(slot.tail_samples() > 0);
        assert!(!slot.set_params(&EffectKind::Delay.default_params()));
        slot.reset();
        let mut fresh = EffectSlot::new(AnyEffect::new(&params));
        fresh.prepare(48_000.0, 128);
        let (mut l, mut r) = (signal(61, 4096), signal(29, 4096));
        let (mut fl, mut fr) = (l.clone(), r.clone());
        slot.process(&mut l, &mut r);
        fresh.process(&mut fl, &mut fr);
        assert_eq!(l, fl);
        assert_eq!(r, fr);
    }
}

#[test]
fn registry_and_slot_callback_paths_never_allocate_reallocate_or_free() {
    for kind in KINDS {
        let mut params = active(kind);
        let wrong = EffectKind::Eq.default_params();
        let mut effect = AnyEffect::new(&params);
        effect.prepare(48_000.0, 512);
        let mut slot = EffectSlot::new(AnyEffect::new(&params));
        slot.prepare(48_000.0, 512);
        let (mut l, mut r) = ([0.1; 512], [-0.1; 512]);
        assert_eq!(
            allocator_calls(|| {
                for n in 0..1000 {
                    let i = n % kind.descriptors().len();
                    let info = &kind.descriptors()[i];
                    assert!(params.set(i, if n % 2 == 0 { info.min } else { info.max }));
                    params = params.sanitized();
                    assert!(effect.set_params(&params));
                    assert!(slot.set_params(&params));
                    assert!(!effect.set_params(&wrong));
                    assert!(!slot.set_params(&wrong));
                    effect.set_tempo(110.0);
                    slot.set_tempo(110.0);
                    slot.set_enabled(n % 11 > 2);
                    slot.set_mix((n % 7) as f32 / 6.0);
                    if n % 29 == 0 {
                        effect.reset();
                        slot.reset();
                    }
                    let frames = [1, 7, 64, 127, 512][n % 5];
                    l.fill(0.1);
                    r.fill(-0.1);
                    effect.process(&mut l[..frames], &mut r[..frames]);
                    slot.process(&mut l[..frames], &mut r[..frames]);
                    std::hint::black_box((
                        effect.latency_samples(),
                        effect.max_latency_samples(48_000.0),
                        effect.tail_samples(),
                        effect.gap_samples(),
                        effect.warm_up_samples(),
                        effect.delay_readiness_samples(),
                        effect.latency_transition_samples_remaining(),
                        effect.gain_reduction(),
                        slot.latency_samples(),
                        slot.tail_samples(),
                        slot.gap_samples(),
                    ));
                }
            }),
            0,
            "{kind:?} callback allocator calls"
        );
    }
    let mut kept = None;
    assert_eq!(allocator_calls(|| kept = Some(vec![1_u8; 64])), 1);
    assert_eq!(allocator_calls(|| drop(kept.take())), 1);
}
