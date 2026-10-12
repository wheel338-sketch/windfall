//! The primitive edits every command is lowered into.
//!
//! Each [`Edit`] carries what it needs to run in both directions, so undo is
//! the same edits applied backward in reverse order and never needs a copy of
//! the whole project.
//!
//! Notes and clips have no stored position: a lane is sorted by
//! [`Note::sort_key`], lanes are sorted by channel id, and clips are sorted by
//! [`Clip::sort_key`]. Putting an item back therefore lands it exactly where
//! it was.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::model::{
    Automation, Channel, ChannelId, Clip, Lane, MixerTrack, Note, Pattern, PatternId,
    PlaylistTrack, Project, ProjectSettings, SampleAsset, SampleId,
};
use crate::patch::Touched;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Forward,
    Backward,
}

/// A value replaced by another.
#[derive(Debug, Clone)]
pub(crate) struct Change<T> {
    pub old: T,
    pub new: T,
}

impl<T> Change<T> {
    /// The value this change leaves behind when applied in `direction`.
    fn result(&self, direction: Direction) -> &T {
        match direction {
            Direction::Forward => &self.new,
            Direction::Backward => &self.old,
        }
    }
}

/// An item put into a list at `index`, or taken out of it from there.
#[derive(Debug, Clone)]
pub(crate) enum ListEdit<T> {
    Insert { index: usize, item: T },
    Remove { index: usize, item: T },
}

impl<T: Clone> ListEdit<T> {
    fn inserts(&self, direction: Direction) -> bool {
        matches!(self, ListEdit::Insert { .. }) == (direction == Direction::Forward)
    }

    fn item(&self) -> &T {
        match self {
            ListEdit::Insert { item, .. } | ListEdit::Remove { item, .. } => item,
        }
    }

    fn apply(&self, list: &mut Vec<T>, direction: Direction) {
        let (ListEdit::Insert { index, item } | ListEdit::Remove { index, item }) = self;
        if self.inserts(direction) {
            list.insert((*index).min(list.len()), item.clone());
        } else if *index < list.len() {
            list.remove(*index);
        }
    }
}

/// An item moved from one index of a list to another.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Move {
    pub from: usize,
    pub to: usize,
}

impl Move {
    fn apply<T>(self, list: &mut Vec<T>, direction: Direction) {
        let (from, to) = match direction {
            Direction::Forward => (self.from, self.to),
            Direction::Backward => (self.to, self.from),
        };
        if from < list.len() && to < list.len() {
            let item = list.remove(from);
            list.insert(to, item);
        }
    }
}

/// The parts of a pattern other than its notes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternInfo {
    pub note_curves: Vec<crate::NoteExpressionCurve>,
    pub name: String,
    pub color: u32,
    pub length_steps: u32,
    pub time_signature: Option<crate::TimeSignature>,
    pub timeline: crate::Timeline,
    pub events: Vec<crate::PatternEventLane>,
}

impl PatternInfo {
    pub(crate) fn of(pattern: &Pattern) -> Self {
        Self {
            note_curves: pattern.note_curves.clone(),
            name: pattern.name.clone(),
            color: pattern.color,
            length_steps: pattern.length_steps,
            time_signature: pattern.time_signature,
            timeline: pattern.timeline.clone(),
            events: pattern.events.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Edit {
    Notebook(Change<crate::Notebook>),
    ArrangementBook(Change<crate::ArrangementBook>),
    Timeline(Change<crate::Timeline>),
    TakeGroups(Change<Vec<crate::AudioTakeGroup>>),
    Settings(Change<ProjectSettings>),
    Plugins(Change<Vec<crate::PluginBinding>>),
    /// Replaces the routing of the effects' separately routable outputs.
    EffectOutputs(Change<Vec<crate::EffectOutputRoute>>),
    Sample(ListEdit<SampleAsset>),
    /// Boxed because a channel is large: an instrument's settings are part
    /// of it.
    Channel(Box<ListEdit<Channel>>),
    MoveChannel(Move),
    /// Replaces the channel that has the same id.
    SetChannel(Box<Change<Channel>>),
    /// The pattern travels with its notes.
    Pattern(ListEdit<Pattern>),
    MovePattern(Move),
    PatternInfo {
        id: PatternId,
        change: Change<PatternInfo>,
    },
    /// Takes `remove` out of a lane and puts `insert` in. A changed note is
    /// in both, with its old data in `remove` and its new data in `insert`.
    /// The lane appears with its first note and goes with its last.
    Notes {
        pattern: PatternId,
        channel: ChannelId,
        remove: Vec<Note>,
        insert: Vec<Note>,
    },
    MixerTrack(ListEdit<MixerTrack>),
    /// Replaces the mixer track that has the same id.
    SetMixerTrack(Box<Change<MixerTrack>>),
    PlaylistTrack(ListEdit<PlaylistTrack>),
    MovePlaylistTrack(Move),
    /// Replaces the playlist track that has the same id.
    SetPlaylistTrack(Change<PlaylistTrack>),
    /// Like `Notes`, for the clips of the playlist.
    Clips {
        remove: Vec<Clip>,
        insert: Vec<Clip>,
    },
    Automation(ListEdit<Automation>),
    /// Replaces the automation that has the same id.
    SetAutomation(Box<Change<Automation>>),
}

impl Edit {
    pub(crate) fn effect_asset_pins(&self) -> windfall_dsp::asset::AssetPins {
        let tracks = match self {
            Self::MixerTrack(edit) => [Some(edit.item()), None],
            Self::SetMixerTrack(change) => [Some(&change.old), Some(&change.new)],
            _ => [None, None],
        };
        windfall_dsp::asset::AssetPins::for_effects(tracks.into_iter().flatten()
            .flat_map(|track| track.effects.iter().map(|effect| &effect.params)))
    }

    pub(crate) fn apply(&self, project: &mut Project, direction: Direction) {
        match self {
            Edit::ArrangementBook(change) => {
                project.playlist.arrangement_book = change.result(direction).clone()
            }
            Edit::Settings(change) => project.settings = change.result(direction).clone(),
            Edit::Notebook(change) => project.notebook = change.result(direction).clone(),
            Edit::Timeline(change) => project.playlist.timeline = change.result(direction).clone(),
            Edit::TakeGroups(change) => {
                project.playlist.take_groups = change.result(direction).clone()
            }
            Edit::Plugins(change) => project.plugins = change.result(direction).clone(),
            Edit::EffectOutputs(change) => {
                project.mixer.effect_outputs = change.result(direction).clone()
            }
            Edit::Sample(edit) => edit.apply(&mut project.samples, direction),
            Edit::Channel(edit) => edit.apply(&mut project.channels, direction),
            Edit::MoveChannel(moved) => moved.apply(&mut project.channels, direction),
            Edit::SetChannel(change) => {
                let result = change.result(direction);
                if let Some(channel) = project.channels.iter_mut().find(|c| c.id == result.id) {
                    *channel = result.clone();
                }
            }
            Edit::Pattern(edit) => edit.apply(&mut project.patterns, direction),
            Edit::MovePattern(moved) => moved.apply(&mut project.patterns, direction),
            Edit::PatternInfo { id, change } => {
                if let Some(pattern) = project.patterns.iter_mut().find(|p| p.id == *id) {
                    let info = change.result(direction);
                    pattern.name = info.name.clone();
                    pattern.color = info.color;
                    pattern.length_steps = info.length_steps;
                    pattern.time_signature = info.time_signature;
                    pattern.timeline = info.timeline.clone();
                    pattern.note_curves = info.note_curves.clone();
                    pattern.events = info.events.clone();
                }
            }
            Edit::Notes {
                pattern,
                channel,
                remove,
                insert,
            } => {
                let (remove, insert) = ordered(remove, insert, direction);
                if let Some(pattern) = project.patterns.iter_mut().find(|p| p.id == *pattern) {
                    splice_notes(pattern, *channel, remove, insert);
                }
            }
            Edit::MixerTrack(edit) => edit.apply(&mut project.mixer.tracks, direction),
            Edit::SetMixerTrack(change) => {
                let result = change.result(direction);
                let tracks = &mut project.mixer.tracks;
                if let Some(track) = tracks.iter_mut().find(|t| t.id == result.id) {
                    *track = result.clone();
                }
            }
            Edit::PlaylistTrack(edit) => edit.apply(&mut project.playlist.tracks, direction),
            Edit::MovePlaylistTrack(moved) => moved.apply(&mut project.playlist.tracks, direction),
            Edit::SetPlaylistTrack(change) => {
                let result = change.result(direction);
                let tracks = &mut project.playlist.tracks;
                if let Some(track) = tracks.iter_mut().find(|t| t.id == result.id) {
                    *track = result.clone();
                }
            }
            Edit::Clips { remove, insert } => {
                let (remove, insert) = ordered(remove, insert, direction);
                let clips = &mut project.playlist.clips;
                splice(clips, remove, insert, |clip| clip.id, Clip::sort_key);
            }
            Edit::Automation(edit) => edit.apply(&mut project.automations, direction),
            Edit::SetAutomation(change) => {
                let result = change.result(direction);
                let automations = &mut project.automations;
                if let Some(automation) = automations.iter_mut().find(|a| a.id == result.id) {
                    *automation = result.clone();
                }
            }
        }
    }

    /// Records in `touched` the sections this edit changes when applied in
    /// `direction`. A pattern that the edit removes is still listed; the
    /// caller drops ids that no longer exist once all edits are in.
    pub(crate) fn mark(&self, direction: Direction, touched: &mut Touched) {
        let mut touch_pattern = |id: PatternId| {
            if !touched.patterns.contains(&id) {
                touched.patterns.push(id);
            }
        };
        match self {
            Edit::Settings(_) => touched.settings = true,
            Edit::Notebook(_) => touched.notebook = true,
            Edit::Timeline(_) => touched.playlist = true,
            Edit::TakeGroups(_) => touched.playlist = true,
            Edit::Plugins(_) => touched.plugins = true,
            Edit::EffectOutputs(_) => touched.mixer = true,
            Edit::Sample(_) => touched.samples = true,
            Edit::Channel(_) | Edit::MoveChannel(_) | Edit::SetChannel(_) => {
                touched.channels = true;
            }
            Edit::Pattern(edit) => {
                if edit.inserts(direction) {
                    touch_pattern(edit.item().id);
                }
                touched.pattern_list = true;
            }
            Edit::MovePattern(_) => touched.pattern_list = true,
            Edit::PatternInfo { id, .. } => touch_pattern(*id),
            Edit::Notes { pattern, .. } => touch_pattern(*pattern),
            Edit::MixerTrack(_) | Edit::SetMixerTrack(_) => touched.mixer = true,
            Edit::ArrangementBook(_)
            | Edit::PlaylistTrack(_)
            | Edit::MovePlaylistTrack(_)
            | Edit::SetPlaylistTrack(_)
            | Edit::Clips { .. } => {
                touched.playlist = true;
            }
            Edit::Automation(_) | Edit::SetAutomation(_) => touched.automations = true,
        }
    }

    /// The sample this edit puts into the pool or takes out of it. Undoing
    /// or redoing the edit brings the sample back exactly as it is held
    /// here.
    pub(crate) fn sample(&self) -> Option<&SampleAsset> {
        match self {
            Edit::Sample(edit) => Some(edit.item()),
            _ => None,
        }
    }

    /// [`Edit::sample`], if it is the sample with this id, to change.
    pub(crate) fn sample_mut(&mut self, id: SampleId) -> Option<&mut SampleAsset> {
        let Edit::Sample(ListEdit::Insert { item, .. } | ListEdit::Remove { item, .. }) = self
        else {
            return None;
        };
        (item.id == id).then_some(item)
    }

    /// True when applying the edit would change nothing.
    pub(crate) fn is_identity(&self) -> bool {
        match self {
            Edit::Settings(change) => change.old == change.new,
            Edit::Notebook(change) => change.old == change.new,
            Edit::Timeline(change) => change.old == change.new,
            Edit::ArrangementBook(change) => change.old == change.new,
            Edit::TakeGroups(change) => change.old == change.new,
            Edit::Plugins(change) => change.old == change.new,
            Edit::EffectOutputs(change) => change.old == change.new,
            Edit::SetChannel(change) => change.old == change.new,
            Edit::SetMixerTrack(change) => change.old == change.new,
            Edit::SetPlaylistTrack(change) => change.old == change.new,
            Edit::SetAutomation(change) => change.old == change.new,
            Edit::PatternInfo { change, .. } => change.old == change.new,
            Edit::MoveChannel(moved)
            | Edit::MovePattern(moved)
            | Edit::MovePlaylistTrack(moved) => moved.from == moved.to,
            Edit::Notes { remove, insert, .. } => remove.is_empty() && insert.is_empty(),
            Edit::Clips { remove, insert } => remove.is_empty() && insert.is_empty(),
            Edit::Sample(_)
            | Edit::Channel(_)
            | Edit::Pattern(_)
            | Edit::MixerTrack(_)
            | Edit::PlaylistTrack(_)
            | Edit::Automation(_) => false,
        }
    }

    /// Folds `next`, which ran right after this edit, into this edit when the
    /// two act on the same thing. Hands `next` back when they do not.
    // An unmatched edit is moved back into history; boxing the common refusal
    // would allocate just to transfer its existing rollback ownership.
    #[allow(clippy::result_large_err)]
    fn absorb(&mut self, next: Edit) -> Result<(), Edit> {
        match (self, next) {
            (Edit::Settings(first), Edit::Settings(second)) => first.new = second.new,
            (Edit::Notebook(first), Edit::Notebook(second)) => first.new = second.new,
            (Edit::ArrangementBook(first), Edit::ArrangementBook(second)) => first.new = second.new,
            (Edit::Timeline(first), Edit::Timeline(second)) => first.new = second.new,
            (Edit::TakeGroups(first), Edit::TakeGroups(second)) => first.new = second.new,
            (Edit::Plugins(first), Edit::Plugins(second)) => first.new = second.new,
            (Edit::EffectOutputs(first), Edit::EffectOutputs(second)) => first.new = second.new,
            (Edit::SetChannel(first), Edit::SetChannel(second))
                if first.new.id == second.new.id =>
            {
                first.new = second.new;
            }
            (Edit::SetMixerTrack(first), Edit::SetMixerTrack(second))
                if first.new.id == second.new.id =>
            {
                first.new = second.new;
            }
            (Edit::SetPlaylistTrack(first), Edit::SetPlaylistTrack(second))
                if first.new.id == second.new.id =>
            {
                first.new = second.new;
            }
            (Edit::SetAutomation(first), Edit::SetAutomation(second))
                if first.new.id == second.new.id =>
            {
                first.new = second.new;
            }
            (
                Edit::PatternInfo { id, change },
                Edit::PatternInfo {
                    id: next_id,
                    change: next_change,
                },
            ) if *id == next_id => change.new = next_change.new,
            (Edit::MoveChannel(first), Edit::MoveChannel(second))
            | (Edit::MovePattern(first), Edit::MovePattern(second))
            | (Edit::MovePlaylistTrack(first), Edit::MovePlaylistTrack(second))
                if first.to == second.from =>
            {
                first.to = second.to;
            }
            (
                Edit::Notes {
                    pattern,
                    channel,
                    remove,
                    insert,
                },
                Edit::Notes {
                    pattern: next_pattern,
                    channel: next_channel,
                    remove: next_remove,
                    insert: next_insert,
                },
            ) if *pattern == next_pattern && *channel == next_channel => {
                compose(remove, insert, next_remove, next_insert, |note| note.id);
            }
            (
                Edit::Clips { remove, insert },
                Edit::Clips {
                    remove: next_remove,
                    insert: next_insert,
                },
            ) => compose(remove, insert, next_remove, next_insert, |clip| clip.id),
            (_, next) => return Err(next),
        }
        Ok(())
    }
}

/// Appends an edit that has just been applied to the list of edits applied
/// before it. Edits that act on the same thing as the last one are folded
/// into it, which keeps a long drag down to one edit however many times the
/// value changed.
pub(crate) fn push(edits: &mut Vec<Edit>, edit: Edit) {
    let edit = match edits.last_mut() {
        Some(last) => match last.absorb(edit) {
            Ok(()) => {
                if last.is_identity() {
                    edits.pop();
                }
                return;
            }
            Err(edit) => edit,
        },
        None => edit,
    };
    if !edit.is_identity() {
        edits.push(edit);
    }
}

fn ordered<'a, T>(remove: &'a [T], insert: &'a [T], direction: Direction) -> (&'a [T], &'a [T]) {
    match direction {
        Direction::Forward => (remove, insert),
        Direction::Backward => (insert, remove),
    }
}

/// Merges a second remove-and-insert edit into the first.
fn compose<T: PartialEq, K: Copy + Eq + Hash>(
    remove: &mut Vec<T>,
    insert: &mut Vec<T>,
    next_remove: Vec<T>,
    next_insert: Vec<T>,
    id: impl Fn(&T) -> K,
) {
    // An item the first edit put in and the second takes out again never
    // needs to exist. Any other item the second edit removes was there
    // before both.
    let inserted: HashSet<K> = insert.iter().map(&id).collect();
    let mut cancelled = HashSet::new();
    for item in next_remove {
        if inserted.contains(&id(&item)) {
            cancelled.insert(id(&item));
        } else {
            remove.push(item);
        }
    }
    if !cancelled.is_empty() {
        insert.retain(|item| !cancelled.contains(&id(item)));
    }
    insert.extend(next_insert);

    // An item that ends up put back exactly as it was is no edit at all, as
    // when a note is dragged away and back.
    let removed: HashMap<K, usize> = remove
        .iter()
        .enumerate()
        .map(|(index, item)| (id(item), index))
        .collect();
    let mut unchanged = HashSet::new();
    insert.retain(|item| match removed.get(&id(item)) {
        Some(&index) if remove[index] == *item => {
            unchanged.insert(id(item));
            false
        }
        _ => true,
    });
    if !unchanged.is_empty() {
        remove.retain(|item| !unchanged.contains(&id(item)));
    }
}

/// Removes and inserts items in a sorted list, keeping it sorted.
fn splice<T: Clone, K: Copy + Eq + Hash, S: Ord>(
    list: &mut Vec<T>,
    remove: &[T],
    insert: &[T],
    id: impl Fn(&T) -> K,
    sort_key: impl Fn(&T) -> S,
) {
    if !remove.is_empty() {
        let removed: HashSet<K> = remove.iter().map(&id).collect();
        list.retain(|item| !removed.contains(&id(item)));
    }
    if !insert.is_empty() {
        list.extend_from_slice(insert);
        list.sort_unstable_by_key(sort_key);
    }
}

fn splice_notes(pattern: &mut Pattern, channel: ChannelId, remove: &[Note], insert: &[Note]) {
    let index = match pattern
        .lanes
        .binary_search_by_key(&channel, |lane| lane.channel)
    {
        Ok(index) => index,
        Err(_) if insert.is_empty() => return,
        Err(index) => {
            let notes = Vec::new();
            pattern.lanes.insert(index, Lane { channel, notes });
            index
        }
    };
    let notes = &mut pattern.lanes[index].notes;
    splice(notes, remove, insert, |note| note.id, Note::sort_key);
    if notes.is_empty() {
        pattern.lanes.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NoteId;

    fn note(id: u32, start: u32) -> Note {
        Note {
            id: NoteId(id),
            start,
            length: 240,
            key: 60,
            velocity: 0.8,
            pan: 0.0,
            expression: Default::default(),
        }
    }

    fn notes_edit(remove: Vec<Note>, insert: Vec<Note>) -> Edit {
        Edit::Notes {
            pattern: PatternId(1),
            channel: ChannelId(2),
            remove,
            insert,
        }
    }

    fn ids(notes: &[Note]) -> Vec<(u32, u32)> {
        let mut ids: Vec<_> = notes.iter().map(|note| (note.id.0, note.start)).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn a_note_moved_twice_folds_into_one_move() {
        let mut edits = Vec::new();
        push(&mut edits, notes_edit(vec![note(1, 0)], vec![note(1, 240)]));
        push(
            &mut edits,
            notes_edit(vec![note(1, 240)], vec![note(1, 480)]),
        );
        let [Edit::Notes { remove, insert, .. }] = edits.as_slice() else {
            panic!("expected one notes edit, got {edits:?}");
        };
        assert_eq!(ids(remove), [(1, 0)]);
        assert_eq!(ids(insert), [(1, 480)]);
    }

    #[test]
    fn a_note_moved_away_and_back_leaves_no_edit() {
        let mut edits = Vec::new();
        push(&mut edits, notes_edit(vec![note(1, 0)], vec![note(1, 240)]));
        push(&mut edits, notes_edit(vec![note(1, 240)], vec![note(1, 0)]));
        assert!(edits.is_empty());
    }

    #[test]
    fn a_note_added_then_removed_leaves_no_edit() {
        let mut edits = Vec::new();
        push(&mut edits, notes_edit(vec![], vec![note(1, 0), note(2, 0)]));
        push(&mut edits, notes_edit(vec![note(1, 0)], vec![]));
        let [Edit::Notes { remove, insert, .. }] = edits.as_slice() else {
            panic!("expected one notes edit, got {edits:?}");
        };
        assert!(remove.is_empty());
        assert_eq!(ids(insert), [(2, 0)]);
    }

    #[test]
    fn edits_on_different_lanes_stay_apart() {
        let mut edits = Vec::new();
        push(&mut edits, notes_edit(vec![], vec![note(1, 0)]));
        push(
            &mut edits,
            Edit::Notes {
                pattern: PatternId(1),
                channel: ChannelId(3),
                remove: vec![],
                insert: vec![note(2, 0)],
            },
        );
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn consecutive_moves_of_one_item_fold() {
        let mut edits = Vec::new();
        push(&mut edits, Edit::MoveChannel(Move { from: 0, to: 2 }));
        push(&mut edits, Edit::MoveChannel(Move { from: 2, to: 1 }));
        let [Edit::MoveChannel(moved)] = edits.as_slice() else {
            panic!("expected one move, got {edits:?}");
        };
        assert_eq!((moved.from, moved.to), (0, 1));

        push(&mut edits, Edit::MoveChannel(Move { from: 1, to: 0 }));
        assert!(edits.is_empty());
    }
}
