use super::*;
#[test]
fn vintage_poly_filter_reaches_history_disk_and_live_offline_pcm() {
    let initial = VintagePolyParams::default();
    acceptance(InstrumentKind::VintagePoly, InstrumentParams::VintagePoly(initial),
        InstrumentParams::VintagePoly(VintagePolyParams { cutoff_hz: 300.0, ..initial }));
}
