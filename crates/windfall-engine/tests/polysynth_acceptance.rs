use super::*;
#[test]
fn vintage_poly_filter_reaches_history_disk_and_live_offline_pcm() {
    let initial = VintagePolyParams::default();
    acceptance(InstrumentKind::VintagePoly, InstrumentParams::VintagePoly(initial),
        InstrumentParams::VintagePoly(VintagePolyParams { cutoff_hz: 300.0, ..initial }));
}

#[test]
fn dualPoly_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::DualPoly.default_params();
    let mut changed = initial;
    let param = InstrumentKind::DualPoly.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::DualPoly.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::DualPoly, initial, changed);
}
