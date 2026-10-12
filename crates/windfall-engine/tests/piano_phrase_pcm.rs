//! Measure the phrase produced by the document's Portamento command.
#[path = "engine/support.rs"]
#[allow(dead_code)]
mod support;
use support::*;
use windfall_core::AudioBuffer;
use windfall_project::{Command, Document, NoteTransform};

#[test]
fn transformed_portamento_phrase_glides_audibly_and_is_partition_independent() {
    const RATE: u32 = 48_000;
    let mut rig = Rig::new();
    let channel = rig.channel(AudioBuffer::from_interleaved(
        RATE,
        1,
        (0..100_000)
            .map(|frame| frame as f32 / 1_000_000.0)
            .collect(),
    ));
    // This phrase measures note gates as well as pitch. An envelope-free
    // sampler intentionally plays its entire one-shot after key-up.
    rig.sampler_mut(channel).envelope = Some(windfall_project::Envelope {
        attack_ms: 0.0,
        decay_ms: 0.0,
        sustain: 1.0,
        release_ms: 0.0,
    });
    rig.note(channel, 0, 480).key = 60;
    rig.note(channel, 960, 960).key = 72;
    let pattern = rig.first_pattern();
    let selected = rig
        .project
        .pattern(pattern)
        .unwrap()
        .lane(channel)
        .unwrap()
        .notes
        .clone();
    let mut document = Document::new(rig.project.clone());
    document
        .dispatch(
            Command::TransformNotes {
                pattern,
                channel,
                notes: selected,
                transform: NoteTransform::Portamento {
                    glide: 240,
                    overlap: 60,
                },
            },
            None,
        )
        .unwrap();
    rig.project = document.project().clone();
    let audio = rig.play(RATE, 45_000, 127);
    // A ramp's playback slope directly measures its pitch ratio, independent
    // of the engine's glide helper. Middle C is unity; its octave is double.
    let slope = |at: usize| (audio[(at + 1) * 2] - audio[at * 2]) * 1_000_000.0;
    assert!((slope(20_000) - 1.0).abs() < 0.01);
    assert!(
        (1.0..1.03).contains(&slope(24_001)),
        "start slope {}",
        slope(24_001)
    );
    assert!((1.3..1.8).contains(&slope(27_000)));
    assert!((slope(30_010) - 2.0).abs() < 0.02);
    for block in [1, 64, 1024] {
        assert_eq!(audio, rig.play(RATE, 45_000, block));
    }
    let saved = windfall_project::file::to_json(document.project()).unwrap();
    rig.project = windfall_project::file::from_json(&saved).unwrap();
    assert_eq!(audio, rig.play(RATE, 45_000, 512));
    document.undo().unwrap();
    rig.project = document.project().clone();
    let original = rig.play(RATE, 45_000, 127);
    assert_eq!(
        original[20_000 * 2],
        0.0,
        "untransformed phrasing has a gap"
    );
}
