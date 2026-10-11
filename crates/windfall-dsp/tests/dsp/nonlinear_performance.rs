use windfall_dsp::asset::{CurvePoint, CurveRef, ShapeRef, publish_audio};
use windfall_dsp::performance::{
    PerformanceRackParams, ScratchParams, TimeTransportParams, VolumeGateParams,
};
use windfall_dsp::{AnyEffect, EffectParams, EffectSlot};

#[test]
fn immutable_curve_source_swaps_and_transient_reloops_have_zero_callback_heap_activity() {
    let a = [CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)];
    let b = [CurvePoint::new(0.0, 1.0), CurvePoint::new(1.0, 0.0)];
    let curves = [CurveRef::new(&a).unwrap(), CurveRef::new(&b).unwrap()];
    let shapes = [ShapeRef::new(&a).unwrap(), ShapeRef::new(&b).unwrap()];
    let source = windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.25; 1024]);
    let reference = publish_audio(&source).unwrap();
    let settings = [
        EffectParams::VolumeGate(VolumeGateParams {
            time_curve: curves[0],
            volume_curve: curves[1],
            loop_beats: 0.125,
            ..Default::default()
        }),
        EffectParams::TimeTransport(TimeTransportParams {
            auto_trigger: true,
            loop_beats: 0.125,
            ..Default::default()
        }),
        EffectParams::Scratch(ScratchParams {
            source_id: 1,
            runtime_source: reference,
            position_curve: curves[0],
            curve_enabled: true,
            ..Default::default()
        }),
        EffectParams::PerformanceRack(PerformanceRackParams {
            xy_enabled: true,
            x: 0.6,
            y: 0.2,
            ..Default::default()
        }),
        EffectParams::Waveshaper(windfall_dsp::drive::WaveshaperParams {
            shape: shapes[0],
            ..Default::default()
        }),
    ];
    for params in settings {
        let mut slot = EffectSlot::new(AnyEffect::new(&params));
        slot.prepare(48000.0, 64);
        slot.set_params(&params);
        let mut left = [0.25; 64];
        let mut right = [-0.2; 64];
        let calls = super::realtime::allocator_calls(|| {
            for n in 0..128 {
                let mut edited = params;
                match &mut edited {
                    EffectParams::VolumeGate(p) => {
                        p.trigger = n % 2 == 0;
                        p.time_curve = curves[n % 2];
                        p.volume_curve = curves[(n + 1) % 2];
                    }
                    EffectParams::Scratch(p) => {
                        p.position_curve = curves[n % 2];
                    }
                    EffectParams::Waveshaper(p) => {
                        p.shape = shapes[n % 2];
                    }
                    EffectParams::TimeTransport(p) => {
                        p.trigger = n % 2 == 0;
                    }
                    EffectParams::PerformanceRack(p) => {
                        p.x = (n % 7) as f32 / 6.0;
                        p.y = 1.0 - p.x;
                    }
                    _ => {}
                }
                slot.set_params(&edited);
                slot.set_tempo(120.0);
                slot.set_transport(true, n as f64 / 100.0);
                slot.process(&mut left, &mut right);
                if n % 17 == 0 {
                    slot.reset();
                }
                left.fill(0.25);
                right.fill(-0.2);
            }
        });
        assert_eq!(calls, 0, "{}", params.kind().name());
    }
}
