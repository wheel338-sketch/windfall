//! Fixed note bytecode is prepared on the control side and baked into the
//! immutable event table. Playback/export consume the same transformed events;
//! no interpreter, allocation or IO is introduced in the audio callback.
use windfall_project::{
    Channel,
    scripting::{NoteTransform, ScriptLibrary, ScriptNote, TransformedNotes},
};

pub(crate) fn prepare(library: &ScriptLibrary, channels: &[Channel]) -> Vec<Option<NoteTransform>> {
    channels
        .iter()
        .map(|c| library.channel_transform(c.id))
        .collect()
}

pub(crate) fn transform(
    program: Option<&NoteTransform>,
    note: &windfall_project::Note,
) -> TransformedNotes {
    let input = ScriptNote::from(note);
    // Checked documents reject this case before publication. An unchecked
    // engine fixture fails open, preserving its original musical event.
    program
        .and_then(|p| p.evaluate_notes(input).ok())
        .unwrap_or_else(|| TransformedNotes::single(input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::scripting::{Program, TransformError};
    #[test]
    fn fixed_transform_success_and_failure_allocate_and_free_nothing() {
        let good = Program::compile("set key key 12 + 0 127 clamp\nemit")
            .unwrap()
            .note_transform(&[])
            .unwrap();
        let bad = Program::compile("set velocity velocity 0 /\nemit")
            .unwrap()
            .note_transform(&[])
            .unwrap();
        let calls = crate::test_alloc::allocator_calls(|| {
            for _ in 0..1024 {
                assert_eq!(good.evaluate(ScriptNote::default()).unwrap().key, 72);
                assert_eq!(
                    bad.evaluate(ScriptNote::default()),
                    Err(TransformError::Arithmetic)
                );
            }
        });
        assert_eq!(calls, 0);
    }

    #[test]
    fn fixed_note_copy_success_and_bounds_failure_allocate_and_free_nothing() {
        let transform = Program::compile("copies 16 1 60\nemit")
            .unwrap()
            .note_transform(&[])
            .unwrap();
        let calls = crate::test_alloc::allocator_calls(|| {
            assert_eq!(
                transform
                    .evaluate_notes(ScriptNote::default())
                    .unwrap()
                    .as_slice()
                    .len(),
                16
            );
            assert_eq!(
                transform
                    .evaluate_notes(ScriptNote {
                        key: 127,
                        ..Default::default()
                    })
                    .unwrap_err(),
                TransformError::NoteBounds
            );
        });
        assert_eq!(calls, 0);
    }
}
