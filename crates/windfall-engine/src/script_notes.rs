//! Fixed note bytecode is prepared on the control side. Constant scores are
//! baked; parameterized scores and raw inputs use the same fixed-array VM and
//! bounded pending queue. Playback and export share their callback path.
use windfall_project::{
    scripting::{NoteTransform, ScriptNote, TransformedNotes},
};

pub(crate) fn prepare(project: &windfall_project::Project) -> Vec<Option<NoteTransform>> {
    project.channels.iter().map(|channel| {
        let library = &project.settings.scripts;
        let mut program = library.channel_transform(channel.id)?;
        program.set_dynamic(library.channel_is_dynamic(project, channel.id));
        Some(program)
    }).collect()
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

/// Global bounded queue; storage is allocated only while attaching a processor.
/// Inputs are FIFO by press ID for repeated original keys. Pending copies retain
/// their evaluated values and channel IDs across script/plan edits.
const INPUTS: usize = 256;
const OUTPUTS: usize = INPUTS * 16;
#[derive(Clone, Copy)]
pub(crate) struct Input {
    pub channel: windfall_project::ChannelId,
    pub key: u8,
    pub origin: crate::voice::Origin,
    pub id: u64,
    pub scripted: bool,
}
#[derive(Clone, Copy)]
struct Pending {
    channel: windfall_project::ChannelId,
    tick: f64,
    note: crate::voice::Note,
}
pub(crate) struct Timing {
    pub input_start: u32,
    pub clock_tick: f64,
    pub gated: bool,
    pub song_origin: Option<f64>,
}

pub(crate) struct Runtime {
    inputs: Vec<Option<Input>>,
    pending: Vec<Option<Pending>>,
    next_id: u64,
    pending_count: usize,
    pub rejected: u32,
}
impl Runtime {
    pub fn new() -> Self {
        Self { inputs: vec![None; INPUTS], pending: vec![None; OUTPUTS], next_id: 1, pending_count: 0, rejected: 0 }
    }
    pub fn active(&self) -> bool { self.pending_count > 0 }
    pub fn register_plain(&mut self, channel: windfall_project::ChannelId, note: crate::voice::Note) -> bool {
        let Some(slot) = self.inputs.iter_mut().find(|slot| slot.is_none()) else {
            self.rejected = self.rejected.saturating_add(1); return false;
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        *slot = Some(Input { channel, key: note.key, origin: note.origin, id, scripted: false });
        true
    }
    pub fn admit(&mut self, plan: &crate::plan::Plan, channel: windfall_project::ChannelId, note: crate::voice::Note,
        outputs: TransformedNotes, timing: Timing) -> bool {
        let Timing { input_start, clock_tick, gated, song_origin } = timing;
        let live = matches!(note.origin, crate::voice::Origin::Live | crate::voice::Origin::Hardware);
        let input_slot = self.inputs.iter().position(Option::is_none);
        if (live && input_slot.is_none())
            || OUTPUTS - self.pending_count < outputs.as_slice().len()
            || outputs.as_slice().iter().any(|o| o.start < input_start) {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        if live { self.inputs[input_slot.unwrap()] = Some(Input { channel, key: note.key, origin: note.origin, id, scripted: true }); }
        self.pending_count += outputs.as_slice().len();
        for output in outputs.as_slice() {
            let at = |offset: f64| song_origin.map_or(clock_tick + offset, |origin|
                origin + plan.warp(plan.unwarp(clock_tick - origin) + offset));
            let tick = at(f64::from(output.start - input_start));
            let mut copy = note;
            copy.input = live.then_some(id);
            copy.key = output.key;
            copy.velocity = output.velocity;
            copy.pan = output.pan;
            copy.end = if gated || !live { at(f64::from(output.start - input_start) + f64::from(output.length)) } else { f64::INFINITY };
            // Scripted pan must remain the base when note-expression curves read it.
            if let Some(source) = &mut copy.source {
                source.start = tick;
                source.end = copy.end;
                source.base_pan = copy.pan;
            }
            *self.pending.iter_mut().find(|s| s.is_none()).unwrap() = Some(Pending { channel, tick, note: copy });
        }
        true
    }
    pub fn pop(&mut self, plan: &crate::plan::Plan, clock: crate::sequencer::Clock, frame: u64) -> Option<crate::voice::Note> {
        loop {
            let (index, pending) = self.pending.iter().enumerate().filter_map(|(i, p)| p.map(|p| (i, p)))
                .filter(|(_, p)| clock.frame_of(p.tick) <= frame).min_by(|(ia, a), (ib, b)| a.tick.total_cmp(&b.tick).then(ia.cmp(ib)))?;
            self.pending[index] = None;
            self.pending_count -= 1;
            if let Some(channel) = plan.channel_ids.get(pending.channel.0) {
                return Some(crate::voice::Note { channel, ..pending.note });
            }
        }
    }
    pub fn release(&mut self, channel: windfall_project::ChannelId, key: u8,
        origin: crate::voice::Origin) -> Option<Input> {
        let (slot, input) = self.inputs.iter().enumerate().filter_map(|(i, p)| p.map(|p| (i, p)))
            .filter(|(_, p)| p.channel == channel && p.key == key && p.origin == origin)
            .min_by_key(|(_, p)| p.id)?;
        self.inputs[slot] = None;
        for output in &mut self.pending {
            if output.is_some_and(|p| p.note.input == Some(input.id)) { *output = None; self.pending_count -= 1; }
        }
        Some(input)
    }
    pub fn cancel_origin(&mut self, origin: crate::voice::Origin) {
        for input in &mut self.inputs {
            if input.is_some_and(|p| p.origin == origin) { *input = None; }
        }
        for output in &mut self.pending {
            if output.is_some_and(|p| p.note.origin == origin) { *output = None; self.pending_count -= 1; }
        }
    }
    pub fn move_ends(&mut self, moved: &impl Fn(f64) -> f64) {
        for pending in self.pending.iter_mut().flatten() {
            pending.tick = moved(pending.tick);
            pending.note.end = moved(pending.note.end);
            if let Some(source) = &mut pending.note.source { source.move_clock(moved); }
        }
    }
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
    #[test]
    fn script_input_capacity_failure_cancel_and_parameter_updates_allocate_and_free_nothing() {
        let mut transform = Program::compile("param transpose 0 12 0\nset key key transpose +\ncopies 16 1 100\nemit")
            .unwrap().note_transform(&[]).unwrap();
        let plan = crate::plan::compile(&windfall_project::Project::new("queue"), &crate::SamplePool::new());
        let mut runtime = Runtime::new();
        let note = crate::voice::Note { input: None, source: None, channel: 0,
            key: 60, velocity: 0.8, pan: 0.0, expression: Default::default(),
            end: f64::INFINITY, origin: crate::voice::Origin::Live };
        let calls = crate::test_alloc::allocator_calls(|| {
            for _ in 0..INPUTS {
                assert!(transform.set_parameter(0, 1.0));
                let outputs = transform.evaluate_notes(ScriptNote::default()).unwrap();
                assert!(runtime.admit(&plan, windfall_project::ChannelId(2), note, outputs,
                    Timing { input_start: 0, clock_tick: 0.0, gated: false, song_origin: None }));
            }
            assert_eq!(runtime.pending.iter().flatten().count(), OUTPUTS);
            let outputs = transform.evaluate_notes(ScriptNote::default()).unwrap();
            assert!(!runtime.admit(&plan, windfall_project::ChannelId(2), note, outputs,
                Timing { input_start: 0, clock_tick: 0.0, gated: false, song_origin: None }));
            assert_eq!(runtime.rejected, 1);
            assert_eq!(runtime.pending.iter().flatten().count(), OUTPUTS);
            assert_eq!(runtime.release(windfall_project::ChannelId(2), 60, crate::voice::Origin::Live).map(|input| input.id), Some(1));
            assert_eq!(runtime.pending.iter().flatten().count(), OUTPUTS-16);
            runtime.cancel_origin(crate::voice::Origin::Live);
            assert_eq!(runtime.inputs.iter().flatten().count(), 0);
            assert!(!runtime.active());
        });
        assert_eq!(calls, 0);
    }

}
