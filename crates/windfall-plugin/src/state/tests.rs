use super::*;
use crate::{GuestEngine, publication::Publication};
use windfall_project::{ChannelSource, InstrumentKind, SampleInstrumentAsset};

#[path = "asset_audit.rs"]
mod asset_audit;
#[path = "../../tests/common/sf2.rs"]
mod sf2;

pub(crate) fn bank_program() -> Program {
    let mut program = crate::clap::tests::program();
    program.samples.clear();
    program.project.samples[0].soundfont = true;
    program.project.samples[0].path = windfall_project::SamplePath::Project("gone.sf2".into());
    program.soundfonts.push(EmbeddedSoundFont {
        id: SampleId(2),
        bytes: sf2::bytes(),
    });
    program.project.channels[0].source = ChannelSource::Instrument {
        params: InstrumentKind::SoundFont.default_params(),
        asset: None,
    };
    program.project.channels[0].sample_instrument = Some(SampleInstrumentAsset::SoundFont {
        path: program.project.samples[0].path.clone(),
        bank: 0,
        program: 0,
        source: Some(SampleId(2)),
    });
    program.project.patterns[0].lanes[0].notes[0].start = 0;
    program.project.patterns[0].lanes[0].notes[0].length = 96;
    program.project.patterns[0].lanes[0].notes[0].key = 60;
    program
}

#[test]
fn raw_sf2_identity_bytes_and_real_instrument_pcm_survive_packet_recall() {
    let program = bank_program();
    let packet = program.encode().unwrap();
    let reopened = Program::decode(&packet).unwrap();
    assert_eq!(reopened.project, program.project);
    assert!(reopened.samples.is_empty());
    assert_eq!(reopened.soundfonts[0].id, SampleId(2));
    assert_eq!(reopened.soundfonts[0].bytes, sf2::bytes());
    let mut pool = SamplePool::new();
    pool.insert_soundfont(
        program.project.channels[0]
            .sample_instrument
            .clone()
            .unwrap(),
        &sf2::bytes(),
    )
    .unwrap();
    let reference = windfall_engine::render(
        &program.project,
        &pool,
        &windfall_engine::RenderOptions::default(),
        &mut |_| true,
    );
    let mut guest = GuestEngine::activate(&reopened, 48_000.0).unwrap();
    assert!(guest.preparation_errors().is_empty());
    assert!(guest.sync_transport(0.0, 120.0, true));
    let mut output = Vec::new();
    for _ in 0..16 {
        output.extend(
            guest
                .render(256)
                .unwrap()
                .as_chunks::<{ crate::OUTPUT_CHANNELS }>()
                .0
                .iter()
                .flat_map(|f| [f[2], f[3]]),
        );
    }
    assert!(output.iter().any(|v| v.abs() > 0.001));
    assert_eq!(output, reference.samples()[..output.len()]);
}

#[test]
fn missing_corrupt_and_budget_refused_banks_report_identity_and_only_silence_the_bank() {
    for corrupt in [false, true] {
        let mut program = bank_program();
        if corrupt {
            program.soundfonts[0].bytes = b"broken SF2".to_vec();
        } else {
            program.soundfonts.clear();
        }
        let program = Program::decode(&program.encode().unwrap()).unwrap();
        assert!(program.validate_soundfont_sources().is_err());
        let mut guest = GuestEngine::activate(&program, 48_000.0).unwrap();
        assert_eq!(guest.preparation_errors().len(), 1);
        assert!(guest.preparation_errors()[0].contains("silent"));
        guest.midi(0, 60, 127);
        assert!(guest.render(256).unwrap().iter().all(|v| *v == 0.0));
        let mut other = program.project.channels[0].clone();
        other.id = windfall_project::ChannelId(7);
        other.name = "unaffected synth".into();
        other.sample_instrument = None;
        other.source = ChannelSource::Instrument {
            params: InstrumentKind::SubtractiveSynth.default_params(),
            asset: None,
        };
        let mut mixed = program;
        mixed.project.channels.push(other);
        let state = Publication::new(mixed);
        state.activate(48_000.0).unwrap();
        assert!(state.preparation_error().unwrap().contains("silent"));
        let engine = unsafe { state.audio() }.unwrap();
        engine.midi(0, 60, 127);
        assert!(engine.render(256).unwrap().iter().all(|v| *v == 0.0));
        engine.midi(1, 60, 127);
        assert!(engine.render(256).unwrap().iter().any(|v| v.abs() > 0.001));
    }
    let program = bank_program();
    let mut pool = SamplePool::with_sample_bank_budget(1);
    let errors = program.prepare_soundfonts(&mut pool);
    assert!(errors[0].contains("Sample-bank memory refused"));
    assert_eq!(pool.sample_bank_retained_bytes(), 0);
}

#[test]
fn schema_one_pcm_packets_still_load_and_hostile_packets_are_bounded() {
    let old = crate::clap::tests::program();
    let packet = serde_json::json!({"schema":1,"project":windfall_project::file::to_json(&old.project).unwrap(),"samples":old.samples});
    let reopened = Program::decode(&serde_json::to_vec(&packet).unwrap()).unwrap();
    assert_eq!(reopened.project, old.project);
    assert!(reopened.soundfonts.is_empty());
    assert!(GuestEngine::activate(&reopened, 48_000.0).is_ok());
    assert!(
        Program::decode(&vec![b' '; MAX_STATE_BYTES + 1])
            .err()
            .unwrap()
            .contains("64 MiB")
    );
    let mut bank = bank_program();
    bank.soundfonts[0].bytes = vec![0; MAX_RAW_BANK_BYTES + 1];
    assert!(bank.encode().err().unwrap().contains("16 MiB"));
    let packet = Packet {
        schema: 2,
        project: windfall_project::file::to_json(&bank.project).unwrap(),
        samples: vec![],
        soundfonts: bank.soundfonts,
        preset_sources: vec![],
    };
    assert!(
        Program::decode(&serde_json::to_vec(&packet).unwrap())
            .err()
            .unwrap()
            .contains("16 MiB")
    );
    let mut bank = bank_program();
    bank.soundfonts[0].bytes = vec![255; MAX_RAW_BANK_BYTES];
    assert!(
        bank.encode().err().unwrap().contains("64 MiB"),
        "encoded byte-array overhead must also be bounded"
    );
    for mutate in [0, 1, 2] {
        let mut bank = bank_program();
        match mutate {
            0 => bank.soundfonts.push(bank.soundfonts[0].clone()),
            1 => bank.soundfonts[0].id = SampleId(999),
            _ => bank.project.samples[0].soundfont = false,
        }
        assert!(bank.encode().is_err());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&bank_program().encode().unwrap()).unwrap();
    value["schema"] = 4.into();
    assert!(Program::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value["schema"] = 1.into();
    assert!(Program::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    value["schema"] = 2.into();
    value["soundfonts"][0]["bytes"] = serde_json::json!([256]);
    assert!(Program::decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn raw_bank_process_note_reset_swap_and_control_retirement_allocate_nothing_on_audio() {
    let program = bank_program();
    let state = Publication::new(program.clone());
    state.activate(48_000.0).unwrap();
    state.load(program).unwrap();
    let (peak, memory) = crate::clap::tests::realtime(|| {
        let guest = unsafe { state.audio() }.unwrap();
        guest.midi(0, 60, 127);
        let mut peak = 0.0f32;
        for _ in 0..64 {
            for sample in guest.render(64).unwrap() {
                peak = peak.max(sample.abs());
            }
        }
        guest.midi(0, 60, 0);
        guest.render(64).unwrap();
        guest.reset();
        let silence = guest.render(64).unwrap().iter().all(|x| *x == 0.0);
        (peak, silence)
    });
    assert_eq!(memory, 0);
    assert!(peak.0 > 0.001);
    assert!(peak.1);
    assert!(state.needs_collection());
    let (_, control_memory) = crate::clap::tests::realtime(|| state.collect());
    assert!(
        control_memory > 0,
        "retired heap owners must actually be destroyed on control"
    );
    let saved = Program::decode(&state.capture().unwrap()).unwrap();
    assert_eq!(saved.soundfonts[0].bytes, sf2::bytes());
    state.deactivate();
}
