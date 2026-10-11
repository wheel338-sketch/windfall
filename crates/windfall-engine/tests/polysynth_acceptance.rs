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

#[test]
fn compactSynth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::CompactSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::CompactSynth.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::CompactSynth.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::CompactSynth, initial, changed);
}

#[test]
fn vintageSaw_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::VintageSaw.default_params();
    let mut changed = initial;
    let param = InstrumentKind::VintageSaw.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::VintageSaw.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::VintageSaw, initial, changed);
}

#[test]
fn pocketSynth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::PocketSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::PocketSynth.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::PocketSynth.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::PocketSynth, initial, changed);
}

#[test]
fn grooveSynth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::GrooveSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::GrooveSynth.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::GrooveSynth.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::GrooveSynth, initial, changed);
}

#[test]
fn hybridSeq_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::HybridSeq.default_params();
    let mut changed = initial;
    let param = InstrumentKind::HybridSeq.descriptors().iter().position(|p| p.id == "cutoffHz").unwrap();
    let info = &InstrumentKind::HybridSeq.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::HybridSeq, initial, changed);
}

#[test]
fn matrixSynth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::MatrixSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::MatrixSynth.descriptors().iter().position(|p| p.id == "fm.0.1").unwrap();
    let info = &InstrumentKind::MatrixSynth.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::MatrixSynth, initial, changed);
}

#[test]
fn packSynth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::PackSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::PackSynth.descriptors().iter().position(|p| p.id == "tone").unwrap();
    let info = &InstrumentKind::PackSynth.descriptors()[param];
    assert!(changed.set(param, if info.default == info.max { info.min } else { info.max }));
    acceptance(InstrumentKind::PackSynth, initial, changed);
}
