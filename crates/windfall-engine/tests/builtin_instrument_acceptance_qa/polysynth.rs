use super::*;
#[test]
fn vintage_poly_filter_reaches_history_disk_and_live_offline_pcm() {
    let initial = VintagePolyParams::default();
    acceptance(
        InstrumentKind::VintagePoly,
        InstrumentParams::VintagePoly(initial),
        InstrumentParams::VintagePoly(VintagePolyParams {
            cutoff_hz: 300.0,
            ..initial
        }),
    );
}

#[test]
fn dual_poly_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::DualPoly.default_params();
    let mut changed = initial;
    let param = InstrumentKind::DualPoly
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::DualPoly.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::DualPoly, initial, changed);
}

#[test]
fn compact_synth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::CompactSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::CompactSynth
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::CompactSynth.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::CompactSynth, initial, changed);
}

#[test]
fn vintage_saw_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::VintageSaw.default_params();
    let mut changed = initial;
    let param = InstrumentKind::VintageSaw
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::VintageSaw.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::VintageSaw, initial, changed);
}

#[test]
fn pocket_synth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::PocketSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::PocketSynth
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::PocketSynth.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::PocketSynth, initial, changed);
}

#[test]
fn groove_synth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::GrooveSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::GrooveSynth
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::GrooveSynth.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::GrooveSynth, initial, changed);
}

#[test]
fn hybrid_seq_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::HybridSeq.default_params();
    let mut changed = initial;
    let param = InstrumentKind::HybridSeq
        .descriptors()
        .iter()
        .position(|p| p.id == "cutoffHz")
        .unwrap();
    let info = &InstrumentKind::HybridSeq.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::HybridSeq, initial, changed);
}

#[test]
fn matrix_synth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::MatrixSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::MatrixSynth
        .descriptors()
        .iter()
        .position(|p| p.id == "fm.0.1")
        .unwrap();
    let info = &InstrumentKind::MatrixSynth.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::MatrixSynth, initial, changed);
}

#[test]
fn pack_synth_musical_edit_reaches_history_disk_and_live_offline_pcm() {
    let initial = InstrumentKind::PackSynth.default_params();
    let mut changed = initial;
    let param = InstrumentKind::PackSynth
        .descriptors()
        .iter()
        .position(|p| p.id == "tone")
        .unwrap();
    let info = &InstrumentKind::PackSynth.descriptors()[param];
    assert!(changed.set(
        param,
        if info.default == info.max {
            info.min
        } else {
            info.max
        }
    ));
    acceptance(InstrumentKind::PackSynth, initial, changed);
}

#[test]
fn hybrid_sequence_edit_reaches_saved_project_and_both_host_paths() {
    let initial = windfall_dsp::polysynth::HybridSeqParams {
        sequencer: true,
        ..Default::default()
    };
    let mut changed = initial;
    changed.steps[1].pitch = 12;
    changed.steps[2].velocity = 0.2;
    changed.steps[3].enabled = false;
    acceptance(
        InstrumentKind::HybridSeq,
        InstrumentParams::HybridSeq(initial),
        InstrumentParams::HybridSeq(changed),
    );
}

#[test]
fn matrix_ring_route_and_modulation_reach_saved_project_and_both_host_paths() {
    let mut initial = windfall_dsp::polysynth::MatrixSynthParams::default();
    initial.operators[1].ratio = 3.0;
    let mut changed = initial;
    changed.ring[0][1] = 1.0;
    changed.lfo_index = 0.5;
    acceptance(
        InstrumentKind::MatrixSynth,
        InstrumentParams::MatrixSynth(initial),
        InstrumentParams::MatrixSynth(changed),
    );
}

#[test]
fn own_pack_engine_selection_reaches_saved_project_and_both_host_paths() {
    let initial = PackSynthParams::default();
    for patch in [
        windfall_dsp::polysynth::PackPatch::EmberGlass,
        windfall_dsp::polysynth::PackPatch::EmberKeys,
        windfall_dsp::polysynth::PackPatch::EmberBell,
    ] {
        acceptance(
            InstrumentKind::PackSynth,
            InstrumentParams::PackSynth(initial),
            InstrumentParams::PackSynth(PackSynthParams { patch, ..initial }),
        );
    }
}
