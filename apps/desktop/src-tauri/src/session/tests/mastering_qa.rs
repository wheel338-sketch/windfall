//! Mastering on export: the file that is written is the master, the review
//! says what the file measures, and nothing is left behind.

use std::path::Path;

use windfall_dsp::mastering::measure;
use windfall_ipc::{
    BitDepth, ExportFormat, ExportOptions, ExportStems, MasteringOptions, PlayMode, StemMode,
    TransportPatch,
};
use windfall_project::{Command, PatternId};

use super::{Rig, SAMPLE_RATE};
use crate::events::Event;

fn options(rig: &Rig, name: &str, mastering: Option<MasteringOptions>) -> ExportOptions {
    ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 4,
        tail_secs: 0.0,
        auto_tail: false,
        mastering,
        ..ExportOptions::default()
    }
}

fn kick_on_the_beat(rig: &Rig) {
    for step in [0, 4, 8, 12] {
        rig.session
            .dispatch(
                Command::ToggleStep {
                    pattern: rig.pattern(),
                    channel: rig.channel(0),
                    step,
                },
                None,
            )
            .unwrap();
    }
}

const ASKED: MasteringOptions = MasteringOptions {
    target_lufs: -18.0,
    ceiling_dbtp: -1.0,
    dynamics: 0.5,
};

fn leftovers(rig: &Rig) -> Vec<String> {
    std::fs::read_dir(rig.folder.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".master") || name.ends_with(".stage"))
        .collect()
}

#[test]
fn a_mastered_export_writes_the_master_and_reviews_what_it_measures() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);

    let plain = options(&rig, "plain.wav", None);
    session.export_audio(plain.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.error, None);
    assert_eq!(done.mastering, None);
    let mix = windfall_codec::decode_file(&plain.path).unwrap();
    rig.events.take();

    let mastered = options(&rig, "mastered.wav", Some(ASKED));
    session.export_audio(mastered.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.error, None);
    assert_eq!(done.fraction, 1.0);
    let review = done.mastering.expect("a mastered export is reviewed");
    let master = windfall_codec::decode_file(&mastered.path).unwrap();
    // As long as the mix and lined up with it: the chain's latency is off.
    assert_eq!(master.frames(), mix.frames());
    assert_eq!(done.files.unwrap()[0].frames, mix.frames() as u64);

    // The review is of the audio in the two files, measured independently.
    let before = measure(mix.samples(), SAMPLE_RATE);
    let after = measure(master.samples(), SAMPLE_RATE);
    assert!((review.before.integrated_lufs - before.integrated_lufs).abs() < 0.01);
    assert!((review.before.true_peak_dbtp - before.true_peak_dbtp).abs() < 0.01);
    assert!((review.after.integrated_lufs - after.integrated_lufs).abs() < 0.01);
    assert!((review.after.true_peak_dbtp - after.true_peak_dbtp).abs() < 0.01);
    assert_eq!((review.target_lufs, review.ceiling_dbtp), (-18.0, -1.0));

    // The master is nearer the target than the mix was, and under the
    // ceiling. When the review says the target was reached, it was.
    let miss = |loudness: f32| (loudness - ASKED.target_lufs).abs();
    assert!(
        miss(after.integrated_lufs) < miss(before.integrated_lufs)
            || miss(before.integrated_lufs) <= 0.3,
        "{before:?} to {after:?}"
    );
    assert!(after.true_peak_dbtp <= -1.0 + 0.3, "{after:?}");
    assert!(after.sample_peak_db <= -1.0 + 0.01, "{after:?}");
    if review.target_reached {
        assert!(miss(after.integrated_lufs) <= 0.31, "{after:?}");
        assert!(after.true_peak_dbtp <= -1.0 + 0.06, "{after:?}");
    }
    assert!(review.passes >= 2 && review.passes <= 7);
    assert_eq!(leftovers(&rig), Vec::<String>::new());

    // Progress never ran backwards on the way.
    let fractions: Vec<f32> = rig
        .events
        .take()
        .into_iter()
        .filter_map(|event| match event {
            Event::ExportProgress(progress) => Some(progress.fraction),
            _ => None,
        })
        .collect();
    assert!(fractions.windows(2).all(|pair| pair[0] <= pair[1]));

    // The same project and settings give the same master.
    let again = options(&rig, "again.wav", Some(ASKED));
    session.export_audio(again.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().mastering, Some(review));
    let second = windfall_codec::decode_file(&again.path).unwrap();
    assert_eq!(second.samples(), master.samples());
}

#[test]
fn mastering_that_cannot_work_is_refused_or_fails_without_leaving_a_file() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);

    let mut with_stems = options(&rig, "stems.wav", Some(ASKED));
    with_stems.stems = Some(ExportStems {
        mode: StemMode::TrackOutputs,
        tracks: None,
        include_mix: true,
        numbered: false,
        folder: false,
    });
    let error = session.export_audio(with_stems).unwrap_err();
    assert!(error.contains("stems"), "{error}");
    let mut broken = options(&rig, "broken.wav", Some(ASKED));
    broken.mastering = Some(MasteringOptions {
        target_lufs: f32::NAN,
        ..ASKED
    });
    let error = session.export_audio(broken).unwrap_err();
    assert!(error.contains("numbers"), "{error}");
    assert!(rig.events.take().iter().all(|event| !matches!(event, Event::ExportProgress(_))));

    // A silent pattern has no loudness to bring anywhere.
    let empty = session
        .dispatch(Command::AddPattern { name: None }, None)
        .unwrap()
        .created[0];
    session
        .transport_set(TransportPatch {
            pattern: Some(PatternId(empty)),
            ..TransportPatch::default()
        })
        .unwrap();
    let silent = options(&rig, "silent.wav", Some(ASKED));
    session.export_audio(silent.clone()).unwrap();
    let done = rig.events.wait_for_export();
    let error = done.error.expect("a silent mix cannot be mastered");
    assert!(error.contains("silent"), "{error}");
    assert_eq!(done.mastering, None);
    assert!(!Path::new(&silent.path).exists());
    assert_eq!(leftovers(&rig), Vec::<String>::new());

    // And the session is free for the next export.
    rig.events.take();
    session
        .export_audio(options(&rig, "after.wav", None))
        .unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
}


#[test]
fn playlist_stems_and_out_of_range_mastering_are_refused_before_starting_a_job() {
    let rig = Rig::new();
    let before = rig.project();
    let mut asked = options(&rig, "playlist.wav", Some(ASKED));
    asked.mode = PlayMode::Song;
    let error = rig.session.export_playlist_tracks(asked, windfall_ipc::ExportPlaylistStems {
        tracks: None, include_mix: true, numbered: false, folder: false,
    }).unwrap_err();
    assert!(error.contains("stems"), "{error}");
    for invalid in [MasteringOptions { target_lufs: -3.0, ..ASKED },
        MasteringOptions { ceiling_dbtp: 1.0, ..ASKED }, MasteringOptions { dynamics: 2.0, ..ASKED }] {
        assert!(rig.session.export_audio(options(&rig, "invalid.wav", Some(invalid))).unwrap_err().contains("Choose a loudness"));
    }
    assert_eq!(rig.project(), before);
    assert!(rig.events.take().iter().all(|event| !matches!(event, Event::ExportProgress(_))));
    assert!(!Path::new(&rig.file("playlist.wav")).exists());
    assert!(leftovers(&rig).is_empty());
}
