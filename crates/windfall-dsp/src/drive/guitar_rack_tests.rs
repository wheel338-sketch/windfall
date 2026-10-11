use super::*;
use crate::Effect;

fn render(params: GuitarRackParams, input: &[f32], block: usize) -> Vec<f32> {
    let mut effect = GuitarRack::default();
    effect.prepare(48_000.0, 512);
    effect.set_params(&params);
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    for (l, r) in left.chunks_mut(block).zip(right.chunks_mut(block)) {
        effect.process(l, r);
    }
    left
}

#[test]
fn simultaneous_pedals_are_a_real_series_rack_with_independent_controls() {
    let input: Vec<_> = (0..12_000)
        .map(|n| 0.3 * (std::f32::consts::TAU * 431.0 * n as f32 / 48_000.0).sin())
        .collect();
    let mut p = GuitarRackParams {
        drive_db: 0.0,
        output_db: 0.0,
        ..Default::default()
    };
    let dry = render(p, &input, 137);
    for pedal in [
        GuitarModel::TubeSoftClip,
        GuitarModel::MidScoop,
        GuitarModel::Cabinet,
    ] {
        p.pedals[pedal as usize].on = true;
    }
    let series = render(p, &input, 137);
    assert!(series.iter().zip(&dry).any(|(a, b)| (a - b).abs() > 0.05));
    for pedal in [
        GuitarModel::TubeSoftClip,
        GuitarModel::MidScoop,
        GuitarModel::Cabinet,
    ] {
        let mut without = p;
        without.pedals[pedal as usize].on = false;
        let audio = render(without, &input, 137);
        assert!(
            series[2048..]
                .iter()
                .zip(&audio[2048..])
                .any(|(a, b)| (a - b).abs() > 1e-3),
            "{pedal:?}"
        );
    }
    for block in [1, 7, 64, 512] {
        assert_eq!(series, render(p, &input, block));
    }
}

#[test]
fn cabinet_selection_and_all_switches_persist_and_new_indices_are_appended() {
    let descriptors = <GuitarRackParams as crate::ParamSet>::descriptors();
    assert_eq!(descriptors[0].id, "model");
    assert_eq!(descriptors[1].id, "driveDb");
    assert_eq!(descriptors[2].id, "outputDb");
    assert_eq!(descriptors.len(), 38);
    let old: GuitarRackParams =
        serde_json::from_str(r#"{"model":"hardFuzz","driveDb":12,"outputDb":-9}"#).unwrap();
    assert!(old.pedal_on(GuitarModel::HardFuzz));
    let mut p = old;
    p.cabinet = Cabinet::Bass;
    p.pedals.fill(PedalParams {
        on: true,
        amount: 0.65,
        tone: 0.3,
    });
    assert_eq!(
        serde_json::from_str::<GuitarRackParams>(&serde_json::to_string(&p).unwrap()).unwrap(),
        p
    );
}

#[test]
fn cabinet_memory_is_bounded_and_prepared_away_from_processing() {
    for rate in [8_000.0, 48_000.0, 384_000.0] {
        for cabinet in Cabinet::ALL {
            assert!(cabinet_response(cabinet, rate).len() <= MAX_CABINET_TAPS);
        }
        let mut effect = GuitarRack::default();
        effect.prepare(rate, 4096);
        let mut p = GuitarRackParams::default();
        p.pedals.fill(PedalParams {
            on: true,
            amount: 1.0,
            tone: 1.0,
        });
        effect.set_params(&p);
        let mut l = [0.1; 256];
        let mut r = [-0.1; 256];
        effect.process(&mut l, &mut r);
        assert!(
            l.iter()
                .chain(&r)
                .all(|x| x.is_finite() && x.abs() <= 1000.0)
        );
    }
}
