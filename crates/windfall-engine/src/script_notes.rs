//! Fixed note bytecode is prepared on the control side and baked into the
//! immutable event table. Playback/export consume the same transformed events;
//! no interpreter, allocation or IO is introduced in the audio callback.
use windfall_project::{Channel, scripting::{NoteTransform, ScriptLibrary, ScriptNote}};

pub(crate) fn prepare(library: &ScriptLibrary, channels: &[Channel]) -> Vec<Option<NoteTransform>> {
    channels.iter().map(|c| library.channel_transform(c.id)).collect()
}

pub(crate) fn transform(program: Option<&NoteTransform>, note: &windfall_project::Note) -> ScriptNote {
    let input = ScriptNote::from(note);
    // Checked documents reject this case before publication. An unchecked
    // engine fixture fails open, preserving its original musical event.
    program.and_then(|p| p.evaluate(input).ok()).unwrap_or(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::scripting::{Program, TransformError};
    #[test]
    fn fixed_transform_success_and_failure_allocate_and_free_nothing() {
        let good = Program::compile("set key key 12 + 0 127 clamp\nemit").unwrap().note_transform(&[]).unwrap();
        let bad = Program::compile("set velocity velocity 0 /\nemit").unwrap().note_transform(&[]).unwrap();
        let calls = crate::test_alloc::allocator_calls(|| {
            for _ in 0..1024 {
                assert_eq!(good.evaluate(ScriptNote::default()).unwrap().key, 72);
                assert_eq!(bad.evaluate(ScriptNote::default()), Err(TransformError::Arithmetic));
            }
        });
        assert_eq!(calls, 0);
    }
}
