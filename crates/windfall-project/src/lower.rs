//! Turns commands into primitive edits.
//!
//! Each command checks everything it needs before it makes its first edit,
//! so a command that fails has changed nothing. A batch runs its commands one
//! after another against the live project, so a later command sees what an
//! earlier one did; when one fails, the edits made so far are taken back.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::automation::AutomationRange;
use crate::check::{reaches, time_signature_problem};
use crate::command::{
    AudioClipPatch, AudioClipUpdate, AutomationPatch, ChannelPatch, ClipInit, ClipPatch,
    ClipUpdate, Command, EffectSlotPatch, MixerTrackPatch, NoteInit, NotePatch, NoteUpdate,
    PatternPatch, PlaylistTrackPatch, SamplerPatch, SettingsPatch,
};
use crate::edit::{self, Change, Direction, Edit, ListEdit, Move, PatternInfo};
use crate::error::CommandError;
use windfall_dsp::ParamInfo;

use crate::model::{
    Automation, AutomationId, AutomationPoint, AutomationTarget, MAX_AUTOMATION_POINTS,
};
use crate::model::{
    Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, DEFAULT_CHANNEL_VOLUME,
    DEFAULT_KEY, DEFAULT_PATTERN_STEPS, DEFAULT_VELOCITY, EffectId, EffectKind, EffectParams,
    EffectSlot, Envelope, InstrumentKind, InstrumentParams, Lane, MAX_EFFECT_SLOTS,
    MAX_ENVELOPE_MS, MAX_GAIN, MAX_KEY, MAX_PATTERN_STEPS, MAX_PATTERN_TICKS, MAX_SONG_TICKS,
    MAX_TEMPO_BPM, MAX_TUNE_SEMITONES, MIN_TEMPO_BPM, MixerTrack, Note, NoteId, PPQ, Pattern,
    PatternId, PlaylistTrack, PlaylistTrackId, Project, SampleAsset, SampleId, SamplePath,
    SamplerSettings, Send, TICKS_PER_STEP, TimeSignature, TrackId, palette_color,
};

type Label = &'static str;

mod playlist_layouts;
/// Preset commands, lowered beside the rest so they share its transaction.
mod preset_apply;

/// What a command did to the project.
pub(crate) struct Outcome {
    /// The edits that were applied, in order. Empty when nothing changed.
    pub edits: Vec<Edit>,
    pub created: Vec<u32>,
    pub label: String,
}

/// Runs a command against the project. On failure the project is exactly as
/// it was, `next_id` included.
pub(crate) fn execute(project: &mut Project, command: Command) -> Result<Outcome, CommandError> {
    let first_id = project.next_id;
    let mut transaction = Transaction {
        project,
        edits: Vec::new(),
        created: Vec::new(),
    };
    let result = transaction.run(command).and_then(|label| {
        if transaction.edits.iter().any(|edit| match edit {
            Edit::Clips { remove, insert } => remove
                .iter()
                .any(|old| !insert.iter().any(|new| new.id == old.id)),
            Edit::PlaylistTrack(ListEdit::Remove { .. })
            | Edit::Sample(ListEdit::Remove { .. }) => true,
            Edit::Channel(edit) => matches!(edit.as_ref(), ListEdit::Remove { .. }),
            _ => false,
        }) {
            transaction.prune_arrangement_references();
        }
        transaction.settle_linked_tracks();
        crate::check::check_arrangement_references(
            transaction.project,
            &transaction.project.playlist.arrangement_book,
        )
        .map_err(CommandError::invalid)?;
        Ok(label)
    });
    match result {
        Ok(label) => {
            let curve_edits: Vec<_> = transaction
                .project
                .patterns
                .iter()
                .filter_map(|pattern| {
                    if pattern.note_curves.is_empty() {
                        return None;
                    }
                    let ids: std::collections::HashSet<_> = pattern
                        .lanes
                        .iter()
                        .flat_map(|lane| &lane.notes)
                        .map(|note| note.id)
                        .collect();
                    let old = PatternInfo::of(pattern);
                    let mut new = old.clone();
                    new.note_curves.retain(|curve| ids.contains(&curve.note));
                    (old != new).then_some(Edit::PatternInfo {
                        id: pattern.id,
                        change: Change { old, new },
                    })
                })
                .collect();
            for edit in curve_edits {
                transaction.push(edit);
            }
            let old = transaction.project.settings.clone();
            let mut new = old.clone();
            new.internal_controllers.retain(|c| crate::internal_controllers::source(transaction.project, c.controller).is_some());
            for config in &mut new.internal_controllers {
                if let Some((_, owner)) = crate::internal_controllers::source(transaction.project, config.controller) {
                    if owner.kind() != EffectKind::FormulaSource { config.expression = None; }
                    if owner.kind() != EffectKind::NoteEnvelope { config.envelope.clear(); }
                    if owner.kind() != EffectKind::ControlSurface { config.controls.clear(); }
                    if owner.kind() != EffectKind::VoltageSource { config.voltage_routes.clear(); }
                }
                if config.note_channel.is_some_and(|id| transaction.project.channel(id).is_none()) { config.note_channel = None; }
            }
            if old != new { transaction.push(Edit::Settings(Change { old, new })); }
            if let Some(links) = crate::controller_links::following(transaction.project) {
                let old = transaction.project.settings.clone();
                let mut new = old.clone();
                new.controller_links = links;
                transaction.push(Edit::Settings(Change { old, new }));
            }
            if transaction.edits.iter().any(|edit| matches!(edit, Edit::Clips { remove, insert } if remove.iter().any(|old| !insert.iter().any(|new| new.id == old.id)))) {
                let old = transaction.project.playlist.take_groups.clone();
                let new = crate::take_groups::prune(transaction.project);
                transaction.push(Edit::TakeGroups(Change { old, new }));
            }
            if let Some(problem) = crate::internal_controllers::links_problem(transaction.project, &transaction.project.settings.controller_links) {
                for edit in transaction.edits.iter().rev() { edit.apply(transaction.project, Direction::Backward); }
                transaction.project.next_id = first_id;
                return Err(CommandError::invalid(problem));
            }
            Ok(Outcome {
                edits: transaction.edits,
                created: transaction.created,
                label,
            })
        }
        Err(error) => {
            for edit in transaction.edits.iter().rev() {
                edit.apply(transaction.project, Direction::Backward);
            }
            transaction.project.next_id = first_id;
            Err(error)
        }
    }
}

struct Transaction<'a> {
    project: &'a mut Project,
    edits: Vec<Edit>,
    created: Vec<u32>,
}

impl Transaction<'_> {
    /// Removing entities retires their associations, while undo restores both.
    fn prune_arrangement_references(&mut self) {
        let old = self.project.playlist.arrangement_book.clone();
        let mut new = old.clone();
        let clips: HashSet<_> = self
            .project
            .playlist
            .clips
            .iter()
            .map(|clip| clip.id)
            .collect();
        let tracks: HashSet<_> = self
            .project
            .playlist
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        for arrangement in &mut new.arrangements {
            arrangement.clips.retain(|id| clips.contains(id));
            arrangement.tracks.retain(|id| tracks.contains(id));
        }
        for group in &mut new.clip_groups {
            group.clips.retain(|id| clips.contains(id));
        }
        new.clip_groups.retain(|group| group.clips.len() >= 2);
        new.track_parents.retain(|track, _| tracks.contains(track));
        new.linked_tracks.retain(|track, kind| {
            tracks.contains(track)
                && match kind {
                    crate::arrangement::TrackKind::Instrument { channel } => {
                        self.project.channel(*channel).is_some()
                    }
                    crate::arrangement::TrackKind::Audio { source } => {
                        self.project.sample(*source).is_some()
                    }
                    crate::arrangement::TrackKind::AudioTrack { mixer } => {
                        self.project.mixer.track(*mixer).is_some()
                    }
                }
        });
        self.push(Edit::ArrangementBook(Change { old, new }));
    }

    /// Newly placed material belongs to the layout the user is editing.
    /// Store membership in the same transaction as the material itself.
    fn enroll_active_arrangement(&mut self, clips: &[ClipId], tracks: &[PlaylistTrackId]) {
        let old = self.project.playlist.arrangement_book.clone();
        let mut new = old.clone();
        if let Some(active) = new.active
            && let Some(arrangement) = new.arrangements.iter_mut().find(|item| item.id == active)
        {
            for id in clips {
                if !arrangement.clips.contains(id) {
                    arrangement.clips.push(*id);
                }
            }
            for id in tracks {
                if !arrangement.tracks.contains(id) {
                    arrangement.tracks.push(*id);
                }
            }
        }
        self.push(Edit::ArrangementBook(Change { old, new }));
    }

    fn push(&mut self, edit: Edit) {
        if edit.is_identity() {
            return;
        }
        edit.apply(self.project, Direction::Forward);
        edit::push(&mut self.edits, edit);
    }

    fn staged_metadata_id(&self) -> Result<(u32, u32), CommandError> {
        let id = self.project.next_id;
        let cursor = id
            .checked_add(1)
            .filter(|_| id != 0)
            .ok_or_else(|| CommandError::invalid("the project has run out of ids"))?;
        Ok((id, cursor))
    }

    fn replace_arrangement_book(
        &mut self,
        next: Result<crate::ArrangementBook, crate::arrangement::ArrangementError>,
        cursor: u32,
    ) -> Result<(), CommandError> {
        let next = next.map_err(|error| CommandError::invalid(error.to_string()))?;
        let mut proposed = self.project.clone();
        proposed.playlist.arrangement_book = next.clone();
        proposed.next_id = cursor;
        proposed.check().map_err(CommandError::invalid)?;
        let old = self.project.playlist.arrangement_book.clone();
        self.push(Edit::ArrangementBook(Change { old, new: next }));
        self.project.next_id = cursor;
        Ok(())
    }

    fn make_clip_unique(&mut self, id: ClipId) -> Result<(), CommandError> {
        use crate::arrangement::{UniqueCopy, UniqueSource, make_unique};
        let old = self
            .project
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id == id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("clip", id))?;
        let source = match old.content {
            ClipContent::Pattern { pattern } => UniqueSource::Pattern(
                self.project
                    .pattern(pattern)
                    .ok_or_else(|| CommandError::not_found("pattern", pattern))?,
            ),
            ClipContent::Audio { sample, .. } => UniqueSource::Audio(
                self.project
                    .sample(sample)
                    .ok_or_else(|| CommandError::not_found("sample", sample))?,
            ),
            ClipContent::Automation { automation } => {
                return self.make_automation_clip_unique(old, automation);
            }
        };
        let mut cursor = self.project.next_id;
        let mut exhausted = false;
        let (_, copy) = make_unique(source, || {
            let id = cursor;
            if id == 0 {
                exhausted = true;
                return 0;
            }
            match cursor.checked_add(1) {
                Some(next) => {
                    cursor = next;
                    id
                }
                None => {
                    exhausted = true;
                    0
                }
            }
        })
        .map_err(|error| CommandError::invalid(error.to_string()))?;
        if exhausted {
            return Err(CommandError::invalid("the project has run out of ids"));
        }
        let mut new = old.clone();
        let mut proposed = self.project.clone();
        let (edit, created) = match copy {
            UniqueCopy::Pattern(item) => {
                new.content = ClipContent::Pattern { pattern: item.id };
                proposed.patterns.push(item.clone());
                (
                    Edit::Pattern(ListEdit::Insert {
                        index: self.project.patterns.len(),
                        item: item.clone(),
                    }),
                    item.id.0,
                )
            }
            UniqueCopy::Audio(item) => {
                if let ClipContent::Audio { sample, .. } = &mut new.content {
                    *sample = item.id;
                }
                proposed.samples.push(item.clone());
                (
                    Edit::Sample(ListEdit::Insert {
                        index: self.project.samples.len(),
                        item: item.clone(),
                    }),
                    item.id.0,
                )
            }
        };
        *proposed
            .playlist
            .clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .unwrap() = new.clone();
        proposed.next_id = cursor;
        proposed.check().map_err(CommandError::invalid)?;
        self.push(edit);
        self.push(Edit::Clips {
            remove: vec![old],
            insert: vec![new],
        });
        self.project.next_id = cursor;
        self.created.push(created);
        Ok(())
    }

    fn allocate(&mut self) -> Result<u32, CommandError> {
        let id = self.project.next_id;
        self.project.next_id = id
            .checked_add(1)
            .ok_or_else(|| CommandError::invalid("the project has run out of ids"))?;
        Ok(id)
    }

    fn run(&mut self, command: Command) -> Result<String, CommandError> {
        let label = match command {
            Command::AddArrangement {
                name,
                clips,
                tracks,
            } => {
                let (id, cursor) = self.staged_metadata_id()?;
                let next = self.project.playlist.arrangement_book.add_arrangement(
                    crate::arrangement::Arrangement {
                        id,
                        name,
                        clips,
                        tracks,
                    },
                );
                self.replace_arrangement_book(next, cursor)?;
                self.created.push(id);
                "Add arrangement"
            }
            Command::RenameArrangement { id, name } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .rename_arrangement(id, name),
                    self.project.next_id,
                )?;
                "Rename arrangement"
            }
            Command::SetArrangementReferences { id, clips, tracks } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .set_arrangement_references(id, clips, tracks),
                    self.project.next_id,
                )?;
                "Set arrangement references"
            }
            Command::SwitchArrangement { id } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .switch_arrangement(id),
                    self.project.next_id,
                )?;
                "Switch arrangement"
            }
            Command::RemoveArrangement { id } => self.remove_arrangement_layout(id)?,
            Command::AddArrangementLayout { name, copy } => {
                self.add_arrangement_layout(name, copy)?
            }
            Command::AddInstrumentTrack {
                name,
                sample,
                instrument,
            } => self.add_instrument_track(name, sample, instrument)?,
            Command::AddAudioTrack { name } => self.add_audio_track(name)?,
            Command::AddTrackGroup { name, parent } => {
                let (id, cursor) = self.staged_metadata_id()?;
                let next = self
                    .project
                    .playlist
                    .arrangement_book
                    .add_track_group(crate::arrangement::TrackGroup { id, name }, parent);
                self.replace_arrangement_book(next, cursor)?;
                self.created.push(id);
                "Add track group"
            }
            Command::RenameTrackGroup { id, name } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .rename_track_group(id, name),
                    self.project.next_id,
                )?;
                "Rename track group"
            }
            Command::MoveTrackGroup { id, parent } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .move_track_group(id, parent),
                    self.project.next_id,
                )?;
                "Move track group"
            }
            Command::MoveTrackToGroup { track, parent } => {
                self.playlist_track_index(track)?;
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .move_track_to_group(track, parent),
                    self.project.next_id,
                )?;
                "Set track group membership"
            }
            Command::RemoveTrackGroup { id } => {
                self.replace_arrangement_book(
                    self.project
                        .playlist
                        .arrangement_book
                        .remove_track_group(id),
                    self.project.next_id,
                )?;
                "Remove track group"
            }
            Command::AddClipGroup { clips } => {
                let (id, cursor) = self.staged_metadata_id()?;
                let next = self
                    .project
                    .playlist
                    .arrangement_book
                    .add_clip_group(crate::arrangement::ClipGroup { id, clips });
                self.replace_arrangement_book(next, cursor)?;
                self.created.push(id);
                "Group clips"
            }
            Command::RemoveClipGroup { id } => {
                self.replace_arrangement_book(
                    self.project.playlist.arrangement_book.remove_clip_group(id),
                    self.project.next_id,
                )?;
                "Ungroup clips"
            }
            Command::LinkTrack { track, kind } => {
                self.playlist_track_index(track)?;
                let next = self
                    .project
                    .playlist
                    .arrangement_book
                    .link_track(track, kind, |kind| match kind {
                        crate::arrangement::TrackKind::Instrument { channel } => {
                            self.project.channel(channel).is_some()
                        }
                        crate::arrangement::TrackKind::Audio { source } => {
                            self.project.sample(source).is_some()
                        }
                        crate::arrangement::TrackKind::AudioTrack { mixer } => self
                            .project
                            .mixer
                            .track(mixer)
                            .is_some_and(|track| !track.current),
                    });
                self.replace_arrangement_book(next, self.project.next_id)?;
                "Link playlist track"
            }
            Command::MakeUnique { clip } => {
                self.make_clip_unique(clip)?;
                "Make clip unique"
            }
            Command::AddNotesWithCurves {
                pattern,
                channel,
                notes,
                curves,
            } => {
                if notes.len() > crate::piano_tools::MAX_TOOL_NOTES
                    || curves.len() > notes.len().saturating_mul(5)
                    || curves.iter().map(|curve| curve.points.len()).sum::<usize>()
                        > crate::note_curves::MAX_PATTERN_CURVE_POINTS
                    || curves
                        .iter()
                        .any(|curve| curve.note_index as usize >= notes.len())
                {
                    return Err(CommandError::invalid(
                        "the curve input note index or insertion count is invalid",
                    ));
                }
                let start = self.created.len();
                self.add_notes(pattern, channel, &notes)?;
                let ids: Vec<_> = self.created[start..].iter().copied().map(NoteId).collect();
                let index = self.pattern_index(pattern)?;
                let old = PatternInfo::of(&self.project.patterns[index]);
                let mut new = old.clone();
                new.note_curves.extend(curves.into_iter().map(|curve| {
                    crate::NoteExpressionCurve {
                        note: ids[curve.note_index as usize],
                        parameter: curve.parameter,
                        points: curve.points,
                    }
                }));
                new.note_curves
                    .sort_by_key(|curve| (curve.note, curve.parameter));
                let mut proposed = self.project.patterns[index].clone();
                proposed.note_curves = new.note_curves.clone();
                crate::note_curves::check(&proposed).map_err(CommandError::invalid)?;
                self.push(Edit::PatternInfo {
                    id: pattern,
                    change: Change { old, new },
                });
                "Add notes with expression curves"
            }
            Command::SetNoteExpressionCurves {
                pattern,
                channel,
                expected,
                expected_curves,
                curves,
            } => {
                self.captured_note_sources(pattern, channel, &expected)?;
                let ids: std::collections::HashSet<_> =
                    expected.iter().map(|note| note.id).collect();
                if expected.is_empty()
                    || curves
                        .iter()
                        .chain(&expected_curves)
                        .any(|curve| !ids.contains(&curve.note))
                {
                    return Err(CommandError::invalid(
                        "choose source notes for the expression curves",
                    ));
                }
                let index = self.pattern_index(pattern)?;
                let before = &self.project.patterns[index];
                let captured: Vec<_> = before
                    .note_curves
                    .iter()
                    .filter(|curve| ids.contains(&curve.note))
                    .cloned()
                    .collect();
                if captured != expected_curves {
                    return Err(CommandError::invalid("the captured note curves changed"));
                }
                let old = PatternInfo::of(before);
                let mut new = old.clone();
                new.note_curves.retain(|curve| !ids.contains(&curve.note));
                new.note_curves.extend(curves);
                new.note_curves
                    .sort_by_key(|curve| (curve.note, curve.parameter));
                let mut proposed = before.clone();
                proposed.note_curves = new.note_curves.clone();
                crate::note_curves::check(&proposed).map_err(CommandError::invalid)?;
                self.push(Edit::PatternInfo {
                    id: pattern,
                    change: Change { old, new },
                });
                "Edit note expression curves"
            }
            Command::SetEffectOutput {
                effect,
                output,
                target,
            } => self.set_effect_output(effect, output, target)?,
            Command::SetPatternEvents {
                pattern,
                target,
                expected,
                points,
            } => self.set_pattern_events(pattern, target, expected, points)?,
            Command::GeneratePatternEventLfo {
                pattern,
                target,
                expected,
                start,
                end,
                resolution,
                lfo,
            } => {
                self.pattern_index(pattern)?;
                let (range, stored) = self.automation_target(&target)?;
                // A pattern with no lane yet starts from what the control has.
                let base = if expected.is_empty() {
                    vec![AutomationPoint {
                        tick: 0,
                        value: range.normalized(stored),
                        curve: 0.0,
                        hold: false,
                    }]
                } else {
                    expected.clone()
                };
                let points = crate::curve_lfo::write_lfo(&base, start, end, resolution, lfo)?;
                self.set_pattern_events(pattern, target, expected, points)?;
                "Write event LFO"
            }
            Command::GenerateNoteCurveLfo {
                pattern,
                channel,
                expected,
                expected_curves,
                parameter,
                origin,
                resolution,
                strength,
                lfo,
            } => {
                let curves = crate::note_curve_lfo::generate(
                    &expected,
                    &expected_curves,
                    parameter,
                    origin,
                    resolution,
                    strength,
                    lfo,
                )?;
                self.run(Command::SetNoteExpressionCurves {
                    pattern,
                    channel,
                    expected,
                    expected_curves,
                    curves,
                })?;
                "Write note curve LFO"
            }
            Command::SetSidechain { from, to, gain } => {
                let index = self.signal_track_index(from)?;
                self.signal_track_index(to)?;
                if from == TrackId::MASTER || from == to {
                    return Err(CommandError::invalid(
                        "Choose an ordinary source and a different sidechain destination",
                    ));
                }
                let old = self.project.mixer.tracks[index].clone();
                let mut new = old.clone();
                let existing = new.sidechains.iter().position(|send| send.target == to);
                match (gain, existing) {
                    (None, Some(index)) => {
                        new.sidechains.remove(index);
                        self.remove_automations_of(|target| {
                            *target
                                == AutomationTarget::SidechainGain {
                                    track: from,
                                    target: to,
                                }
                        });
                    }
                    (None, None) => {}
                    (Some(gain), index) => {
                        let gain = clamped("Sidechain gain", gain, 0.0, MAX_GAIN)?;
                        if index.is_none() && reaches(&self.project.mixer, to, from) {
                            return Err(CommandError::invalid(
                                "The sidechain would make routing loop back on itself",
                            ));
                        }
                        if let Some(index) = index {
                            new.sidechains[index].gain = gain;
                        } else {
                            new.sidechains.push(Send { target: to, gain });
                        }
                    }
                }
                self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
                "Route sidechain input"
            }
            Command::ApplyMixerTrackPreset {
                id,
                expected,
                preset,
                name_color,
            } => {
                preset.validate().map_err(CommandError::invalid)?;
                let index = self.mixer_track_index(id)?;
                if self.project.mixer.tracks[index] != expected {
                    return Err(CommandError::invalid(
                        "The destination track changed; reload the preset",
                    ));
                }
                let removed: HashSet<_> = expected.effects.iter().map(|slot| slot.id).collect();
                self.remove_automations_of(|target| {
                    removed.iter().any(|effect| targets_effect(target, *effect))
                });
                let mut effects = preset.effects.clone();
                let mut remap = std::collections::HashMap::new();
                for effect in &mut effects {
                    let fresh = EffectId(self.allocate()?);
                    remap.insert(effect.id, fresh);
                    effect.id = fresh;
                    self.created.push(fresh.0);
                }
                let mut new = expected.clone();
                preset.settings_into(&mut new, name_color);
                new.effects = effects;
                self.push(Edit::SetMixerTrack(Box::new(Change { old: expected, new })));
                for mut plugin in preset.plugins {
                    if let crate::PluginTarget::Effect { effect } = plugin.target {
                        plugin.target = crate::PluginTarget::Effect {
                            effect: remap[&effect],
                        };
                    }
                    self.bind_plugin(plugin);
                }
                "Load mixer track preset"
            }
            Command::MoveMixerTracks {
                expected,
                ids,
                before,
            } => {
                let order: Vec<_> = self
                    .project
                    .mixer
                    .tracks
                    .iter()
                    .map(|track| track.id)
                    .collect();
                if order != expected {
                    return Err(CommandError::invalid(
                        "The mixer order changed before the tracks could move",
                    ));
                }
                if ids.is_empty() || ids.len() > crate::MAX_MIXER_SIGNAL_TRACKS {
                    return Err(CommandError::invalid("Choose tracks to move"));
                }
                let picked: HashSet<_> = ids.iter().copied().collect();
                if picked.len() != ids.len() {
                    return Err(CommandError::invalid(
                        "Move selection contains duplicate tracks",
                    ));
                }
                for id in &ids {
                    let track = &self.project.mixer.tracks[self.signal_track_index(*id)?];
                    if track.id == TrackId::MASTER {
                        return Err(CommandError::invalid("Master is pinned and cannot move"));
                    }
                }
                if let Some(id) = before {
                    self.signal_track_index(id)?;
                    if id == TrackId::MASTER || picked.contains(&id) {
                        return Err(CommandError::invalid(
                            "Choose an unselected insert as the move destination",
                        ));
                    }
                }
                let moving: Vec<_> = order
                    .iter()
                    .copied()
                    .filter(|id| picked.contains(id))
                    .collect();
                let mut wanted: Vec<_> = order
                    .into_iter()
                    .filter(|id| !picked.contains(id))
                    .collect();
                let at = before.map_or(wanted.len(), |id| {
                    wanted.iter().position(|other| *other == id).unwrap()
                });
                wanted.splice(at..at, moving);
                for (to, id) in wanted.iter().enumerate().skip(1) {
                    let from = self.mixer_track_index(*id)?;
                    if from != to {
                        let item = self.project.mixer.tracks[from].clone();
                        self.push(Edit::MixerTrack(ListEdit::Remove {
                            index: from,
                            item: item.clone(),
                        }));
                        self.push(Edit::MixerTrack(ListEdit::Insert { index: to, item }));
                    }
                }
                "Move mixer tracks"
            }
            Command::SetTrackParam { id, param, value } => {
                use windfall_dsp::ParamSet;
                let index = self.mixer_track_index(id)?;
                let info = windfall_dsp::TrackParams::descriptors()
                    .get(param as usize)
                    .ok_or_else(|| no_setting("Track EQ and stereo", param))?;
                if !value.is_finite() {
                    return Err(CommandError::invalid(
                        "Track parameter must be a finite number",
                    ));
                }
                self.change_track(index, |track| {
                    track.processing.set(param as usize, value);
                });
                return Ok(format!("Change {}", info.name));
            }
            Command::EnsureCurrentMixerTrack => {
                if let Some(track) = self.project.mixer.tracks.iter().find(|track| track.current) {
                    self.created.push(track.id.0);
                } else {
                    let id = TrackId(self.allocate()?);
                    let mut item = new_mixer_track(id, "Current".into(), 0x8b8b9c);
                    item.current = true;
                    item.output = None;
                    let index = self.project.mixer.tracks.len();
                    self.push(Edit::MixerTrack(ListEdit::Insert { index, item }));
                    self.created.push(id.0);
                }
                "Show Current mixer utility"
            }
            Command::SetTrackExternalOutput { id, route } => {
                if let Some(route) = &route {
                    route.check().map_err(CommandError::invalid)?;
                }
                let index = self.mixer_track_index(id)?;
                if self.project.mixer.tracks[index].current && route.is_some() {
                    return Err(CommandError::invalid(
                        "Current is a utility strip and has no hardware output",
                    ));
                }
                self.change_track(index, |track| track.external_output = route);
                "Route hardware output"
            }
            Command::CreateAudioTakeGroup { name, lanes } => {
                if self.project.playlist.take_groups.len() >= crate::take_groups::MAX_TAKE_GROUPS {
                    return Err(CommandError::invalid(
                        "Saved take groups exceed the project capacity",
                    ));
                }
                let group = crate::AudioTakeGroup {
                    id: self.allocate()?,
                    name,
                    lanes,
                    comp: Vec::new(),
                };
                crate::take_groups::check_group(self.project, &group)
                    .map_err(CommandError::invalid)?;
                let used: HashSet<_> = self
                    .project
                    .playlist
                    .take_groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .lanes
                            .iter()
                            .flat_map(|lane| &lane.takes)
                            .map(|take| take.clip)
                            .chain(group.comp.iter().copied())
                    })
                    .collect();
                if group
                    .lanes
                    .iter()
                    .flat_map(|lane| &lane.takes)
                    .any(|take| used.contains(&take.clip))
                {
                    return Err(CommandError::invalid(
                        "An audio clip already belongs to a take group",
                    ));
                }
                let old = self.project.playlist.take_groups.clone();
                let mut new = old.clone();
                self.created.push(group.id);
                new.push(group);
                self.push(Edit::TakeGroups(Change { old, new }));
                "Group audio takes"
            }
            Command::RenameAudioTakeGroup { id, name } => {
                let old = self.project.playlist.take_groups.clone();
                let mut new = old.clone();
                let group = new
                    .iter_mut()
                    .find(|group| group.id == id)
                    .ok_or_else(|| CommandError::invalid("Take group no longer exists"))?;
                group.name = name;
                crate::take_groups::check_group(self.project, group)
                    .map_err(CommandError::invalid)?;
                self.push(Edit::TakeGroups(Change { old, new }));
                "Rename take group"
            }
            Command::RemoveAudioTakeGroup { id } => {
                let old = self.project.playlist.take_groups.clone();
                let mut new = old.clone();
                if !new.iter().any(|group| group.id == id) {
                    return Err(CommandError::invalid("Take group no longer exists"));
                }
                new.retain(|group| group.id != id);
                self.push(Edit::TakeGroups(Change { old, new }));
                "Ungroup audio takes"
            }
            Command::AuditionAudioTakeGroup { id, pass } => {
                let group = self
                    .project
                    .playlist
                    .take_groups
                    .iter()
                    .find(|group| group.id == id)
                    .ok_or_else(|| CommandError::invalid("Take group no longer exists"))?;
                if pass.is_some_and(|pass| {
                    !group
                        .lanes
                        .iter()
                        .all(|lane| lane.takes.iter().any(|take| take.pass == pass))
                }) || pass.is_some() && group.lanes.is_empty()
                    || pass.is_none() && group.comp.is_empty()
                {
                    return Err(CommandError::invalid(
                        "Choose a pass present on every lane, or an existing composite",
                    ));
                }
                let updates = group
                    .lanes
                    .iter()
                    .flat_map(|lane| &lane.takes)
                    .map(|take| ClipUpdate {
                        id: take.clip,
                        patch: ClipPatch {
                            muted: Some(pass != Some(take.pass)),
                            ..Default::default()
                        },
                    })
                    .chain(group.comp.iter().map(|id| ClipUpdate {
                        id: *id,
                        patch: ClipPatch {
                            muted: Some(pass.is_some()),
                            ..Default::default()
                        },
                    }))
                    .collect::<Vec<_>>();
                self.update_clips(&updates)?;
                "Audition recording takes"
            }
            Command::CompAudioTakeGroup {
                expected,
                sources,
                ranges,
                name,
                fade_ticks,
                mute_sources,
                replace_comp,
            } => {
                let prepared = crate::audio_comp::prepare_group(
                    self.project,
                    &expected,
                    &sources,
                    &ranges,
                    fade_ticks,
                )?;
                if replace_comp {
                    let tracks: HashSet<_> = self
                        .project
                        .playlist
                        .clips
                        .iter()
                        .filter(|clip| expected.comp.contains(&clip.id))
                        .map(|clip| clip.track)
                        .collect();
                    self.remove_clips(&expected.comp)?;
                    for track in tracks {
                        if !self
                            .project
                            .playlist
                            .clips
                            .iter()
                            .any(|clip| clip.track == track)
                        {
                            self.remove_playlist_track(track)?;
                        }
                    }
                } else {
                    let updates = expected
                        .comp
                        .iter()
                        .map(|id| ClipUpdate {
                            id: *id,
                            patch: ClipPatch {
                                muted: Some(true),
                                ..Default::default()
                            },
                        })
                        .collect::<Vec<_>>();
                    self.update_clips(&updates)?;
                }
                let mut output = Vec::new();
                for (lane, clips) in prepared {
                    self.add_playlist_track(Some(format!("{name} · {lane}")), None)?;
                    let track = PlaylistTrackId(*self.created.last().expect("created comp lane"));
                    let clips = clips
                        .into_iter()
                        .map(|clip| ClipInit {
                            track,
                            start: clip.start,
                            length: Some(clip.length),
                            offset: Some(clip.offset),
                            muted: Some(false),
                            content: clip.content,
                        })
                        .collect::<Vec<_>>();
                    let before = self.created.len();
                    self.add_clips(&clips)?;
                    output.extend(self.created[before..].iter().copied().map(ClipId));
                }
                if mute_sources {
                    let updates = sources
                        .iter()
                        .map(|clip| ClipUpdate {
                            id: clip.id,
                            patch: ClipPatch {
                                muted: Some(true),
                                ..Default::default()
                            },
                        })
                        .collect::<Vec<_>>();
                    self.update_clips(&updates)?;
                }
                let old = self.project.playlist.take_groups.clone();
                let mut new = old.clone();
                let group = new
                    .iter_mut()
                    .find(|group| group.id == expected.id)
                    .expect("captured take group");
                group.comp = output;
                self.push(Edit::TakeGroups(Change { old, new }));
                "Comp multitrack takes"
            }
            Command::CompAudioClips {
                sources,
                segments,
                destination,
                name,
                fade_ticks,
                mute_sources,
            } => {
                let prepared =
                    crate::audio_comp::prepare(self.project, &sources, &segments, fade_ticks)?;
                let track = if let Some(track) = destination {
                    self.playlist_track_index(track)?;
                    track
                } else {
                    self.add_playlist_track(Some(name), None)?;
                    PlaylistTrackId(*self.created.last().expect("created comp track"))
                };
                let clips = prepared
                    .into_iter()
                    .map(|clip| ClipInit {
                        track,
                        start: clip.start,
                        length: Some(clip.length),
                        offset: Some(clip.offset),
                        muted: Some(false),
                        content: clip.content,
                    })
                    .collect::<Vec<_>>();
                self.add_clips(&clips)?;
                if mute_sources {
                    let updates = sources
                        .iter()
                        .map(|source| ClipUpdate {
                            id: source.id,
                            patch: ClipPatch {
                                muted: Some(true),
                                ..Default::default()
                            },
                        })
                        .collect::<Vec<_>>();
                    self.update_clips(&updates)?;
                }
                "Comp audio takes"
            }
            Command::EditPatternTimeline {
                pattern,
                expected,
                expected_signature,
                edit,
            } => self.edit_pattern_timeline(pattern, expected, expected_signature, edit)?,
            Command::AddMeterChange { tick, signature } => {
                let id = self.allocate()?;
                let mut timeline = self.project.playlist.timeline.clone();
                timeline.meters.push(crate::MeterChange {
                    id: crate::MeterChangeId(id),
                    tick,
                    signature,
                });
                self.set_timeline(timeline)?;
                self.created.push(id);
                "Add meter change"
            }
            Command::UpdateMeterChange {
                id,
                tick,
                signature,
            } => {
                let mut timeline = self.project.playlist.timeline.clone();
                let change = timeline
                    .meters
                    .iter_mut()
                    .find(|m| m.id == id)
                    .ok_or_else(|| CommandError::invalid("the meter change does not exist"))?;
                *change = crate::MeterChange {
                    id,
                    tick,
                    signature,
                };
                self.set_timeline(timeline)?;
                "Change meter"
            }
            Command::RemoveMeterChange { id } => {
                let mut timeline = self.project.playlist.timeline.clone();
                if !timeline.meters.iter().any(|m| m.id == id) {
                    return Err(CommandError::invalid("the meter change does not exist"));
                }
                timeline.meters.retain(|m| m.id != id);
                self.set_timeline(timeline)?;
                "Remove meter change"
            }
            Command::AddTimelineMarker { tick, name, kind } => {
                let id = self.allocate()?;
                let mut timeline = self.project.playlist.timeline.clone();
                timeline.markers.push(crate::TimelineMarker {
                    id: crate::TimelineMarkerId(id),
                    tick,
                    name,
                    kind,
                });
                self.set_timeline(timeline)?;
                self.created.push(id);
                "Add timeline marker"
            }
            Command::UpdateTimelineMarker { marker } => {
                let mut timeline = self.project.playlist.timeline.clone();
                let current = timeline
                    .markers
                    .iter_mut()
                    .find(|m| m.id == marker.id)
                    .ok_or_else(|| CommandError::invalid("the marker does not exist"))?;
                *current = marker;
                self.set_timeline(timeline)?;
                "Change timeline marker"
            }
            Command::RemoveTimelineMarker { id } => {
                let mut timeline = self.project.playlist.timeline.clone();
                if !timeline.markers.iter().any(|m| m.id == id) {
                    return Err(CommandError::invalid("the marker does not exist"));
                }
                timeline.markers.retain(|m| m.id != id);
                self.set_timeline(timeline)?;
                "Remove timeline marker"
            }
            Command::AddPluginInstrument { mut plugin } => {
                plugin.validate().map_err(CommandError::invalid)?;
                let first = self.created.len();
                self.add_channel(
                    Some(plugin.name.clone()),
                    None,
                    Some(InstrumentKind::SubtractiveSynth),
                    None,
                    None,
                )?;
                plugin.target = crate::PluginTarget::Instrument {
                    channel: ChannelId(self.created[first]),
                };
                self.bind_plugin(plugin);
                "Add plugin instrument"
            }
            Command::AddPluginEffect { track, mut plugin } => {
                plugin.validate().map_err(CommandError::invalid)?;
                self.add_effect(track, EffectKind::Eq, None)?;
                plugin.target = crate::PluginTarget::Effect {
                    effect: EffectId(
                        *self
                            .created
                            .last()
                            .ok_or_else(|| CommandError::invalid("the effect was not created"))?,
                    ),
                };
                self.bind_plugin(plugin);
                "Add plugin effect"
            }
            Command::SetPluginParam { target, id, value } => {
                self.plugin_param(target, id, value)?;
                "Change plugin parameter"
            }
            Command::SetPluginSidechainInput { target, input } => {
                if !matches!(target, crate::PluginTarget::Effect { .. }) {
                    return Err(CommandError::invalid("sidechain inputs belong to effects"));
                }
                let mut plugins = self.project.plugins.clone();
                let plugin = plugins
                    .iter_mut()
                    .find(|plugin| plugin.target == target)
                    .ok_or_else(|| CommandError::invalid("the plugin is not in this project"))?;
                if input.is_some_and(|index| {
                    index >= 64
                        || (!plugin.auxiliary_inputs.is_empty()
                            && !plugin
                                .auxiliary_inputs
                                .iter()
                                .any(|port| port.index == index))
                }) {
                    return Err(CommandError::invalid(
                        "the plugin has no such auxiliary input",
                    ));
                }
                plugin.sidechain_input = input;
                plugin.validate().map_err(CommandError::invalid)?;
                self.push(Edit::Plugins(Change {
                    old: self.project.plugins.clone(),
                    new: plugins,
                }));
                "Choose plugin sidechain input"
            }
            Command::SetPluginState { target, state } => {
                let mut plugins = self.project.plugins.clone();
                let plugin = plugins
                    .iter_mut()
                    .find(|plugin| plugin.target == target)
                    .ok_or_else(|| CommandError::invalid("the plugin is not in this project"))?;
                plugin.state = state;
                plugin.validate().map_err(CommandError::invalid)?;
                self.push(Edit::Plugins(Change {
                    old: self.project.plugins.clone(),
                    new: plugins,
                }));
                "Save plugin state"
            }
            Command::UpdateSettings { patch } => self.update_settings(patch)?,
            Command::ReplaceNotebook { notebook } => {
                let new = notebook.normalized()?;
                self.push(Edit::Notebook(Change {
                    old: self.project.notebook.clone(),
                    new,
                }));
                "Save notebook"
            }
            Command::ApplyChannelPreset {
                id,
                expected,
                preset,
                sample,
                audio,
                name_color,
            } => self.apply_channel_preset(id, expected, preset, sample, audio, name_color)?,
            Command::ApplyPluginPreset {
                target,
                expected,
                preset,
            } => self.apply_plugin_preset(target, expected, preset)?,
            Command::SetControllerLinks { links } => {
                if let Some(problem) =
                    crate::controller_links::links_problem(&links, Some(self.project))
                {
                    return Err(CommandError::invalid(problem));
                }
                let old = self.project.settings.clone();
                let mut new = old.clone();
                new.controller_links = links;
                self.push(Edit::Settings(Change { old, new }));
                "Change controller links"
            }
            Command::SetInternalControllerSettings { settings } => {
                if let Some(problem) = crate::internal_controllers::settings_problem(self.project, &settings) {
                    return Err(CommandError::invalid(problem));
                }
                let old = self.project.settings.clone();
                let mut new = old.clone();
                new.internal_controllers = settings;
                self.push(Edit::Settings(Change { old, new }));
                "Configure internal controllers"
            }
            Command::AddSample { name, path } => self.add_sample(name, path)?,
            Command::RemoveSample { id } => self.remove_sample(id)?,
            Command::AddChannel {
                name,
                sample,
                instrument,
                index,
                mixer_track,
            } => self.add_channel(name, sample, instrument, index, mixer_track)?,
            Command::RemoveChannel { id } => self.remove_channel(id)?,
            Command::DuplicateChannel { id } => self.duplicate_channel(id)?,
            Command::MoveChannel { id, index } => self.move_channel(id, index)?,
            Command::SetChannelVoiceSettings { id, settings } => {
                let old = self
                    .project
                    .channel(id)
                    .ok_or_else(|| CommandError::not_found("channel", id))?
                    .clone();
                let settings = settings.sanitized();
                crate::layer::check_settings(&self.project.channels, id, &settings)
                    .map_err(CommandError::invalid)?;
                let new = Channel {
                    voice: settings,
                    ..old.clone()
                };
                self.push(Edit::SetChannel(Box::new(Change { old, new })));
                "Set channel voice settings"
            }
            Command::UpdateChannel { id, patch } => self.update_channel(id, patch)?,
            Command::SetChannelExpressionMap { id, map } => {
                let old = self
                    .project
                    .channel(id)
                    .ok_or_else(|| CommandError::not_found("channel", id))?
                    .clone();
                let new = crate::expression_map::applied(&old, map)?;
                self.push(Edit::SetChannel(Box::new(Change { old, new })));
                "Set channel expression mapping"
            }
            Command::SetChannelGroup { channels, group } => {
                self.set_channel_group(&channels, &group)?;
                "Assign channel group"
            }
            Command::RenameChannelGroup { name, new_name } => {
                let name = checked_group_name(&name)?;
                let new_name = checked_group_name(&new_name)?;
                if name.is_empty() || new_name.is_empty() {
                    return Err(CommandError::invalid(
                        "a named group cannot have an empty name",
                    ));
                }
                let channels = self.channels_in_group(&name)?;
                self.set_channel_group(&channels, &new_name)?;
                "Rename channel group"
            }
            Command::RemoveChannelGroup { name } => {
                let name = checked_group_name(&name)?;
                if name.is_empty() {
                    return Err(CommandError::invalid(
                        "the ungrouped category cannot be removed",
                    ));
                }
                let channels = self.channels_in_group(&name)?;
                self.set_channel_group(&channels, "")?;
                "Remove channel group"
            }
            Command::SetChannelSample { id, sample } => self.set_channel_sample(id, sample)?,
            Command::UpdateSampler { id, patch } => self.update_sampler(id, patch)?,
            Command::SetSamplerEnvelope { id, envelope } => {
                self.set_sampler_envelope(id, envelope)?
            }
            Command::SetInstrumentParam {
                channel,
                param,
                value,
            } => return self.set_instrument_param(channel, param, value),
            Command::SetInstrumentParams { channel, params } => {
                self.set_instrument_params(channel, params)?
            }
            Command::AddPattern { name } => self.add_pattern(name)?,
            Command::RemovePattern { id } => self.remove_pattern(id)?,
            Command::DuplicatePattern { id } => self.duplicate_pattern(id)?,
            Command::MovePattern { id, index } => self.move_pattern(id, index)?,
            Command::UpdatePattern { id, patch } => self.update_pattern(id, patch)?,
            Command::ToggleStep {
                pattern,
                channel,
                step,
            } => self.toggle_step(pattern, channel, step)?,
            Command::AddNotes {
                pattern,
                channel,
                notes,
            } => self.add_notes(pattern, channel, &notes)?,
            Command::FillStepRange {
                pattern,
                channel,
                length_steps,
                expected,
                start_step,
                end_step,
                replace,
                notes,
            } => self.fill_step_range(
                pattern,
                channel,
                length_steps,
                &expected,
                start_step..end_step,
                replace,
                &notes,
            )?,
            Command::RemoveNotes {
                pattern,
                channel,
                notes,
            } => self.remove_notes(pattern, channel, &notes)?,
            Command::UpdateNotes {
                pattern,
                channel,
                updates,
            } => self.update_notes(pattern, channel, &updates)?,
            Command::UpdateCapturedNotes {
                pattern,
                channel,
                expected,
                updates,
            } => {
                self.captured_notes(pattern, channel, &expected, &updates)?;
                self.update_notes(pattern, channel, &updates)?
            }
            Command::ClearLane { pattern, channel } => self.clear_lane(pattern, channel)?,
            Command::TransformNotes {
                pattern,
                channel,
                notes,
                transform,
            } => self.transform_notes(pattern, channel, &notes, transform)?,
            Command::AddMixerTrack { name } => self.add_mixer_track(name)?,
            Command::RemoveMixerTrack { id } => self.remove_mixer_track(id)?,
            Command::UpdateMixerTrack { id, patch } => self.update_mixer_track(id, patch)?,
            Command::SetTrackOutput { id, output } => self.set_track_output(id, output)?,
            Command::SetSend { from, to, gain } => self.set_send(from, to, gain)?,
            Command::AddEffect { track, kind, index } => self.add_effect(track, kind, index)?,
            Command::RemoveEffect { track, effect } => self.remove_effect(track, effect)?,
            Command::MoveEffect {
                track,
                effect,
                to_track,
                index,
            } => self.move_effect(track, effect, to_track, index)?,
            Command::UpdateEffect {
                track,
                effect,
                patch,
            } => self.update_effect(track, effect, patch)?,
            Command::SetEffectParam {
                track,
                effect,
                param,
                value,
            } => return self.set_effect_param(track, effect, param, value),
            Command::SetEffectParams {
                track,
                effect,
                params,
            } => self.set_effect_params(track, effect, params)?,
            Command::DuplicateEffect { track, effect } => self.duplicate_effect(track, effect)?,
            Command::ReplaceEffect {
                track,
                effect,
                kind,
            } => self.replace_effect(track, effect, kind)?,
            Command::AddPlaylistTrack { name, index } => self.add_playlist_track(name, index)?,
            Command::RemovePlaylistTrack { id } => self.remove_playlist_track(id)?,
            Command::UpdatePlaylistTrack { id, patch } => self.update_playlist_track(id, patch)?,
            Command::MovePlaylistTrack { id, index } => self.move_playlist_track(id, index)?,
            Command::AddClips { clips } => self.add_clips(&clips)?,
            Command::RemoveClips { clips } => self.remove_clips(&clips)?,
            Command::UpdateClips { updates } => self.update_clips(&updates)?,
            Command::UpdateAudioClips { updates } => self.update_audio_clips(&updates)?,
            Command::AddAutomation {
                name,
                target,
                points,
            } => self.add_automation(name, target, points)?,
            Command::RemoveAutomation { id } => self.remove_automation(id)?,
            Command::UpdateAutomation { id, patch } => self.update_automation(id, patch)?,
            Command::SetAutomationPoints { id, points } => {
                self.set_automation_points(id, points)?
            }
            Command::GenerateAutomationLfo {
                expected,
                start,
                end,
                resolution,
                lfo,
            } => {
                if self.project.automation(expected.id) != Some(&expected) {
                    return Err(CommandError::invalid(
                        "the captured automation changed; reopen the LFO tool",
                    ));
                }
                let points =
                    crate::curve_lfo::write_lfo(&expected.points, start, end, resolution, lfo)?;
                self.set_automation_points(expected.id, points)?;
                "Write automation LFO"
            }
            Command::DuplicateAutomation { id } => self.duplicate_automation(id)?,
            Command::Batch { label, commands } => {
                let mut first = None;
                for command in commands {
                    let label = self.run(command)?;
                    first.get_or_insert(label);
                }
                // A blank label would be a blank line in the history menu.
                let label = label.filter(|label| !label.trim().is_empty());
                return Ok(label.or(first).unwrap_or_else(|| "Edit".to_owned()));
            }
        };
        self.prune_plugins();
        self.prune_effect_outputs();
        Ok(label.to_owned())
    }

    /// Effect output routes follow their effects: an edit that deletes or
    /// replaces an effect, deletes a target, or moves an effect to where
    /// its route would loop, retires the route in the same undo step.
    fn prune_effect_outputs(&mut self) {
        let new = crate::effect_outputs::pruned(&self.project.mixer);
        self.push(Edit::EffectOutputs(Change {
            old: self.project.mixer.effect_outputs.clone(),
            new,
        }));
    }

    fn set_effect_output(
        &mut self,
        effect: EffectId,
        output: u8,
        target: Option<TrackId>,
    ) -> Result<Label, CommandError> {
        let mixer = &self.project.mixer;
        let home = crate::effect_outputs::home(mixer, effect)
            .ok_or_else(|| CommandError::invalid("the effect is not in this project"))?;
        let kind = home
            .effects
            .iter()
            .find(|slot| slot.id == effect)
            .map(EffectSlot::kind)
            .ok_or_else(|| CommandError::invalid("the effect is not in this project"))?;
        if usize::from(output) >= crate::effect_outputs::output_count(kind) {
            return Err(CommandError::invalid(
                "This effect has no such output to route",
            ));
        }
        let from = home.id;
        if let Some(to) = target {
            if from == TrackId::MASTER || home.current {
                return Err(CommandError::invalid(
                    "An effect on this track cannot send to another track",
                ));
            }
            self.signal_track_index(to)?;
            if to == from {
                return Err(CommandError::invalid(
                    "Choose a different track to receive this output",
                ));
            }
            // The route being replaced is not part of the loop test.
            let mut without = self.project.mixer.clone();
            without.effect_outputs = crate::effect_outputs::with_route(
                &without.effect_outputs,
                effect,
                output,
                None,
            );
            if reaches(&without, to, from) {
                return Err(CommandError::invalid(
                    "The route would make routing loop back on itself",
                ));
            }
        }
        let old = self.project.mixer.effect_outputs.clone();
        let new = crate::effect_outputs::with_route(&old, effect, output, target);
        self.push(Edit::EffectOutputs(Change { old, new }));
        Ok(if target.is_some() {
            "Route effect output"
        } else {
            "Disconnect effect output"
        })
    }

    fn bind_plugin(&mut self, plugin: crate::PluginBinding) {
        let mut plugins = self.project.plugins.clone();
        plugins.retain(|other| other.target != plugin.target);
        plugins.push(plugin);
        self.push(Edit::Plugins(Change {
            old: self.project.plugins.clone(),
            new: plugins,
        }));
    }

    fn plugin_param(
        &mut self,
        target: crate::PluginTarget,
        id: u32,
        value: f32,
    ) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::invalid("the plugin parameter must be finite"));
        }
        let mut plugins = self.project.plugins.clone();
        let plugin = plugins
            .iter_mut()
            .find(|plugin| plugin.target == target)
            .ok_or_else(|| CommandError::invalid("the plugin is not in this project"))?;
        let param = plugin
            .parameters
            .iter_mut()
            .find(|param| param.id == id && !param.read_only)
            .ok_or_else(|| CommandError::invalid("the plugin parameter cannot be changed"))?;
        let value = value.clamp(param.min, param.max);
        param.value = if param.stepped {
            value.round().clamp(param.min, param.max)
        } else {
            value
        };
        self.push(Edit::Plugins(Change {
            old: self.project.plugins.clone(),
            new: plugins,
        }));
        Ok(())
    }

    fn prune_plugins(&mut self) {
        let plugins = self
            .project
            .plugins
            .iter()
            .filter(|plugin| match plugin.target {
                crate::PluginTarget::Instrument { channel } => {
                    self.project.channels.iter().any(|c| {
                        c.id == channel && matches!(c.source, ChannelSource::Instrument { .. })
                    })
                }
                crate::PluginTarget::Effect { effect } => self
                    .project
                    .mixer
                    .tracks
                    .iter()
                    .any(|track| track.effects.iter().any(|slot| slot.id == effect)),
            })
            .cloned()
            .collect();
        self.push(Edit::Plugins(Change {
            old: self.project.plugins.clone(),
            new: plugins,
        }));
    }

    fn set_timeline(&mut self, mut timeline: crate::Timeline) -> Result<(), CommandError> {
        timeline.meters.sort_by_key(|m| m.tick);
        timeline.markers.sort_by_key(|m| (m.tick, m.id));
        timeline
            .check(self.project.settings.time_signature, self.project.next_id)
            .map_err(CommandError::invalid)?;
        self.push(Edit::Timeline(Change {
            old: self.project.playlist.timeline.clone(),
            new: timeline,
        }));
        Ok(())
    }

    fn update_settings(&mut self, patch: SettingsPatch) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename project"),
                (patch.author.is_some(), "Change project author"),
                (patch.genre.is_some(), "Change project genre"),
                (patch.comments.is_some(), "Change project comments"),
                (patch.tempo_bpm.is_some(), "Change tempo"),
                (patch.time_signature.is_some(), "Change time signature"),
                (patch.swing.is_some(), "Change swing"),
                (patch.pan_law.is_some(), "Change panning law"),
                (patch.timebase_ppq.is_some(), "Change timebase"),
            ],
            "Change project settings",
        );
        let old = self.project.settings.clone();
        let mut new = old.clone();
        if let Some(name) = patch.name {
            new.name = name;
        }
        for (value, target, field, cap) in [
            (patch.author, &mut new.author, "author", 256),
            (patch.genre, &mut new.genre, "genre", 128),
            (patch.comments, &mut new.comments, "comments", 16_384),
        ] {
            if let Some(value) = value {
                let value = value.trim();
                if value.len() > cap {
                    return Err(CommandError::invalid(format!(
                        "the project {field} is longer than {cap} bytes"
                    )));
                }
                *target = value.to_owned();
            }
        }
        if let Some(tempo) = patch.tempo_bpm {
            if tempo.is_nan() {
                return Err(CommandError::invalid("the tempo is not a number"));
            }
            new.tempo_bpm = tempo.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM);
        }
        if let Some(signature) = patch.time_signature {
            if let Some(problem) = time_signature_problem(signature) {
                return Err(CommandError::invalid(problem));
            }
            new.time_signature = signature;
        }
        if let Some(swing) = patch.swing {
            new.swing = clamped("the swing", swing, 0.0, 1.0)?;
        }
        if let Some(law) = patch.pan_law {
            new.pan_law = law;
        }
        if let Some(ppq) = patch.timebase_ppq {
            if !crate::timebase::is_timebase(ppq) {
                return Err(CommandError::invalid(format!(
                    "a timebase of {ppq} ticks per quarter note is not one of {:?}",
                    crate::timebase::TIMEBASES
                )));
            }
            new.timebase_ppq = ppq;
        }
        self.push(Edit::Settings(Change { old, new }));
        Ok(label)
    }

    fn add_sample(&mut self, name: String, path: SamplePath) -> Result<Label, CommandError> {
        let label = "Add sample";
        if let Some(problem) = path.problem() {
            return Err(CommandError::invalid(format!(
                "the sample path is not valid: {problem}"
            )));
        }
        if let Some(existing) = self.project.samples.iter().find(|s| s.path == path) {
            self.created.push(existing.id.0);
            return Ok(label);
        }
        let id = SampleId(self.allocate()?);
        self.push(Edit::Sample(ListEdit::Insert {
            index: self.project.samples.len(),
            item: SampleAsset { id, name, path },
        }));
        self.created.push(id.0);
        Ok(label)
    }

    fn remove_sample(&mut self, id: SampleId) -> Result<Label, CommandError> {
        let index = self.sample_index(id)?;
        let channels = &self.project.channels;
        let user = channels.iter().find(|c| c.source.sample() == Some(id));
        if let Some(channel) = user {
            return Err(CommandError::invalid(format!(
                "the sample is still used by the channel \"{}\"",
                channel.name
            )));
        }
        let playlist = &self.project.playlist;
        let clip = playlist
            .clips
            .iter()
            .find(|c| c.content.sample() == Some(id));
        if let Some(clip) = clip {
            let track = playlist.tracks.iter().find(|t| t.id == clip.track);
            return Err(CommandError::invalid(format!(
                "the sample is still used by an audio clip on the playlist track \"{}\"",
                track.map_or("", |track| track.name.as_str())
            )));
        }
        let item = self.project.samples[index].clone();
        self.push(Edit::Sample(ListEdit::Remove { index, item }));
        Ok("Delete sample")
    }

    fn add_channel(
        &mut self,
        name: Option<String>,
        sample: Option<SampleId>,
        instrument: Option<InstrumentKind>,
        index: Option<u32>,
        mixer_track: Option<TrackId>,
    ) -> Result<Label, CommandError> {
        let sample_name = match sample {
            Some(id) => Some(self.project.samples[self.sample_index(id)?].name.clone()),
            None => None,
        };
        if sample.is_some() && instrument.is_some() {
            return Err(CommandError::invalid(
                "a channel plays a sample or an instrument, not both",
            ));
        }
        if let Some(track) = mixer_track {
            self.signal_track_index(track)?;
        }
        let source = match instrument {
            Some(kind) => ChannelSource::Instrument {
                params: kind.default_params(),
            },
            None => ChannelSource::Sampler(SamplerSettings {
                sample,
                ..SamplerSettings::default()
            }),
        };
        let name = name.or(sample_name).unwrap_or_else(|| {
            let fallback = instrument.map_or("Sampler", InstrumentKind::name);
            fallback.to_owned()
        });
        let count = self.project.channels.len();
        let color = palette_color(count);
        let index = index.map_or(count, |index| (index as usize).min(count));

        let id = ChannelId(self.allocate()?);
        let track_count = self.project.mixer.tracks.len();
        let (track, made_track) = match mixer_track {
            Some(track) => (track, false),
            None if self
                .project
                .mixer
                .tracks
                .iter()
                .filter(|track| !track.current)
                .count()
                < crate::MAX_MIXER_SIGNAL_TRACKS =>
            {
                (TrackId(self.allocate()?), true)
            }
            // A full mixer must not stop the user adding channels.
            None => (TrackId::MASTER, false),
        };
        if made_track {
            self.push(Edit::MixerTrack(ListEdit::Insert {
                index: track_count,
                item: new_mixer_track(track, name.clone(), color),
            }));
        }
        self.push(Edit::Channel(Box::new(ListEdit::Insert {
            index,
            item: Channel {
                id,
                name,
                group: String::new(),
                voice: Default::default(),
                expression_map: Default::default(),
                timing: crate::ChannelTiming::default(),
                color,
                volume: DEFAULT_CHANNEL_VOLUME,
                pan: 0.0,
                muted: false,
                solo: false,
                mixer_track: track,
                source,
            },
        })));
        self.created.push(id.0);
        if made_track {
            self.created.push(track.0);
        }
        Ok("Add channel")
    }

    fn remove_channel(&mut self, id: ChannelId) -> Result<Label, CommandError> {
        let index = self.channel_index(id)?;
        self.remove_automations_of(|target| match *target {
            AutomationTarget::ChannelVolume { channel }
            | AutomationTarget::ChannelPan { channel }
            | AutomationTarget::InstrumentParam { channel, .. } => channel == id,
            _ => false,
        });
        for (pattern, notes) in self.lanes_of(id) {
            self.push(Edit::Notes {
                pattern,
                channel: id,
                remove: notes,
                insert: Vec::new(),
            });
        }
        // No layer keeps the deleted channel as a child; undo restores both.
        for (parent, new) in crate::layer::without_child(&self.project.channels, id) {
            let old = self.project.channels[parent].clone();
            self.push(Edit::SetChannel(Box::new(Change { old, new })));
        }
        let item = self.project.channels[index].clone();
        self.push(Edit::Channel(Box::new(ListEdit::Remove { index, item })));
        Ok("Delete channel")
    }

    fn duplicate_channel(&mut self, id: ChannelId) -> Result<Label, CommandError> {
        let index = self.channel_index(id)?;
        let original = &self.project.channels[index];
        let name = copy_name(
            &original.name,
            self.project.channels.iter().map(|c| c.name.as_str()),
        );
        let mut copy = Channel {
            name,
            ..original.clone()
        };
        let mut lanes = self.lanes_of(id);

        copy.id = ChannelId(self.allocate()?);
        let mut copied_curves = Vec::new();
        for (pattern, notes) in &mut lanes {
            for note in notes {
                let source = note.id;
                note.id = NoteId(self.allocate()?);
                if let Some(before) = self.project.pattern(*pattern) {
                    for curve in before
                        .note_curves
                        .iter()
                        .filter(|curve| curve.note == source)
                    {
                        let mut curve = curve.clone();
                        curve.note = note.id;
                        copied_curves.push((*pattern, curve));
                    }
                }
            }
        }
        let copy_id = copy.id;
        self.push(Edit::Channel(Box::new(ListEdit::Insert {
            index: index + 1,
            item: copy,
        })));
        for (pattern, notes) in lanes {
            self.push(Edit::Notes {
                pattern,
                channel: copy_id,
                remove: Vec::new(),
                insert: notes,
            });
        }
        self.created.push(copy_id.0);
        let patterns: std::collections::HashSet<_> =
            copied_curves.iter().map(|(pattern, _)| *pattern).collect();
        for id in patterns {
            let index = self.pattern_index(id)?;
            let old = PatternInfo::of(&self.project.patterns[index]);
            let mut new = old.clone();
            new.note_curves.extend(
                copied_curves
                    .iter()
                    .filter(|(pattern, _)| *pattern == id)
                    .map(|(_, curve)| curve.clone()),
            );
            new.note_curves
                .sort_by_key(|curve| (curve.note, curve.parameter));
            let mut proposed = self.project.patterns[index].clone();
            proposed.note_curves = new.note_curves.clone();
            crate::note_curves::check(&proposed).map_err(CommandError::invalid)?;
            self.push(Edit::PatternInfo {
                id,
                change: Change { old, new },
            });
        }
        if let Some(mut plugin) = self
            .project
            .plugin(crate::PluginTarget::Instrument { channel: id })
            .cloned()
        {
            plugin.target = crate::PluginTarget::Instrument { channel: copy_id };
            self.bind_plugin(plugin);
        }
        Ok("Duplicate channel")
    }

    fn move_channel(&mut self, id: ChannelId, index: u32) -> Result<Label, CommandError> {
        let from = self.channel_index(id)?;
        let to = (index as usize).min(self.project.channels.len() - 1);
        self.push(Edit::MoveChannel(Move { from, to }));
        Ok("Move channel")
    }

    fn update_channel(
        &mut self,
        id: ChannelId,
        patch: ChannelPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename channel"),
                (patch.group.is_some(), "Assign channel group"),
                (patch.timing.is_some(), "Change channel note timing"),
                (patch.color.is_some(), "Change channel color"),
                (patch.volume.is_some(), "Change channel volume"),
                (patch.pan.is_some(), "Change channel pan"),
                (patch.muted == Some(true), "Mute channel"),
                (patch.muted == Some(false), "Unmute channel"),
                (patch.solo == Some(true), "Solo channel"),
                (patch.solo == Some(false), "Unsolo channel"),
                (patch.mixer_track.is_some(), "Route channel"),
            ],
            "Change channel",
        );
        let old = self.project.channels[self.channel_index(id)?].clone();
        let mut new = old.clone();
        if let Some(name) = patch.name {
            new.name = name;
        }
        if let Some(group) = patch.group {
            new.group = checked_group_name(&group)?;
        }
        if let Some(timing) = patch.timing {
            timing.validate().map_err(CommandError::invalid)?;
            new.timing = timing;
        }
        if let Some(color) = patch.color {
            new.color = checked_color(color)?;
        }
        if let Some(volume) = patch.volume {
            new.volume = clamped("the volume", volume, 0.0, MAX_GAIN)?;
        }
        if let Some(pan) = patch.pan {
            new.pan = clamped("the pan", pan, -1.0, 1.0)?;
        }
        if let Some(muted) = patch.muted {
            new.muted = muted;
        }
        if let Some(solo) = patch.solo {
            new.solo = solo;
        }
        if let Some(track) = patch.mixer_track {
            self.signal_track_index(track)?;
            new.mixer_track = track;
        }
        // The track made for the channel goes by the channel's name, for
        // as long as the channel stays on it.
        let renamed = new.name != old.name && new.mixer_track == old.mixer_track;
        if let Some(track) = self.track_named_after(&old).filter(|_| renamed) {
            let name = new.name.clone();
            self.change_track(track, |track| track.name = name);
        }
        self.push(Edit::SetChannel(Box::new(Change { old, new })));
        Ok(label)
    }

    fn channels_in_group(&self, name: &str) -> Result<Vec<ChannelId>, CommandError> {
        let channels: Vec<_> = self
            .project
            .channels
            .iter()
            .filter(|channel| channel.group == name)
            .map(|channel| channel.id)
            .collect();
        if channels.is_empty() {
            return Err(CommandError::invalid("the channel group no longer exists"));
        }
        Ok(channels)
    }

    fn set_channel_group(
        &mut self,
        channels: &[ChannelId],
        group: &str,
    ) -> Result<(), CommandError> {
        let group = checked_group_name(group)?;
        let mut seen = HashSet::new();
        let mut indices = Vec::with_capacity(channels.len());
        for id in channels {
            if seen.insert(*id) {
                indices.push(self.channel_index(*id)?);
            }
        }
        for index in indices {
            let old = self.project.channels[index].clone();
            let new = Channel {
                group: group.clone(),
                ..old.clone()
            };
            self.push(Edit::SetChannel(Box::new(Change { old, new })));
        }
        Ok(())
    }

    fn set_channel_sample(
        &mut self,
        id: ChannelId,
        sample: Option<SampleId>,
    ) -> Result<Label, CommandError> {
        let index = self.channel_index(id)?;
        let mut sampler = self.sampler(index)?;
        if let Some(sample) = sample {
            self.sample_index(sample)?;
        }
        sampler.sample = sample;
        self.set_sampler(index, sampler);
        Ok("Change channel sample")
    }

    fn update_sampler(
        &mut self,
        id: ChannelId,
        patch: SamplerPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.root_key.is_some(), "Change root key"),
                (patch.stretch.is_some(), "Prepare sampler stretch"),
                (patch.tune.is_some(), "Change tuning"),
                (patch.gain.is_some(), "Change sample gain"),
                (
                    patch.start.is_some() || patch.end.is_some(),
                    "Change sample range",
                ),
                (patch.reverse.is_some(), "Reverse sample"),
                (patch.loop_mode.is_some(), "Change sample loop mode"),
                (patch.loop_crossfade.is_some(), "Change loop crossfade"),
                (
                    patch.loop_start.is_some() || patch.loop_end.is_some(),
                    "Change sample loop range",
                ),
                (patch.cut_self.is_some(), "Toggle cut itself"),
                (patch.cut_group.is_some(), "Change cut group"),
            ],
            "Change sampler",
        );
        let index = self.channel_index(id)?;
        let mut sampler = self.sampler(index)?;
        if let Some(stretch) = patch.stretch {
            stretch.validate().map_err(CommandError::invalid)?;
            sampler.stretch = stretch;
        }
        if let Some(root_key) = patch.root_key {
            sampler.root_key = checked_key(root_key)?;
        }
        if let Some(tune) = patch.tune {
            let range = MAX_TUNE_SEMITONES;
            sampler.tune = clamped("the tuning", tune, -range, range)?;
        }
        if let Some(gain) = patch.gain {
            sampler.gain = clamped("the sample gain", gain, 0.0, MAX_GAIN)?;
        }
        if let Some(start) = patch.start {
            sampler.start = clamped("the sample start", start, 0.0, 1.0)?;
        }
        if let Some(end) = patch.end {
            sampler.end = clamped("the sample end", end, 0.0, 1.0)?;
        }
        if sampler.start >= sampler.end {
            return Err(CommandError::invalid(
                "the sample start must come before its end",
            ));
        }
        if let Some(reverse) = patch.reverse {
            sampler.reverse = reverse;
        }
        if let Some(mode) = patch.loop_mode {
            sampler.loop_mode = mode;
        }
        if let Some(amount) = patch.loop_crossfade {
            sampler.loop_crossfade = if amount.is_finite() {
                amount.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        if let Some(start) = patch.loop_start {
            if !start.is_finite() {
                return Err(CommandError::invalid("the loop start is not a number"));
            }
            sampler.loop_start = clamped("the loop start", start, 0.0, 1.0)?;
        }
        if let Some(end) = patch.loop_end {
            if !end.is_finite() {
                return Err(CommandError::invalid("the loop end is not a number"));
            }
            sampler.loop_end = clamped("the loop end", end, 0.0, 1.0)?;
        }
        if sampler.loop_start >= sampler.loop_end {
            return Err(CommandError::invalid(
                "the loop start must come before its end",
            ));
        }
        if let Some(cut_self) = patch.cut_self {
            sampler.cut_self = cut_self;
        }
        if let Some(cut_group) = patch.cut_group {
            sampler.cut_group = cut_group;
        }
        self.set_sampler(index, sampler);
        Ok(label)
    }

    fn set_sampler_envelope(
        &mut self,
        id: ChannelId,
        envelope: Option<Envelope>,
    ) -> Result<Label, CommandError> {
        let index = self.channel_index(id)?;
        let mut sampler = self.sampler(index)?;
        sampler.envelope = match envelope {
            Some(envelope) => Some(Envelope {
                attack_ms: clamped("the attack", envelope.attack_ms, 0.0, MAX_ENVELOPE_MS)?,
                decay_ms: clamped("the decay", envelope.decay_ms, 0.0, MAX_ENVELOPE_MS)?,
                sustain: clamped("the sustain", envelope.sustain, 0.0, 1.0)?,
                release_ms: clamped("the release", envelope.release_ms, 0.0, MAX_ENVELOPE_MS)?,
            }),
            None => None,
        };
        self.set_sampler(index, sampler);
        Ok("Change envelope")
    }

    fn set_instrument_param(
        &mut self,
        channel: ChannelId,
        param: u32,
        value: f32,
    ) -> Result<String, CommandError> {
        let target = crate::PluginTarget::Instrument { channel };
        if let Some(plugin) = self.project.plugin(target) {
            let parameter = plugin
                .parameters
                .get(param as usize)
                .ok_or_else(|| CommandError::invalid("the plugin parameter does not exist"))?;
            let id = parameter.id;
            let label = format!("Change {}", parameter.name);
            self.plugin_param(target, id, value)?;
            return Ok(label);
        }
        let index = self.channel_index(channel)?;
        let mut params = self.instrument(index)?;
        let kind = params.kind();
        if !params.set(param as usize, checked_setting(value)?) {
            return Err(no_setting(kind.name(), param));
        }
        self.set_instrument(index, params);
        Ok(setting_label(kind.descriptors(), param))
    }

    fn set_instrument_params(
        &mut self,
        channel: ChannelId,
        params: InstrumentParams,
    ) -> Result<Label, CommandError> {
        if self
            .project
            .plugin(crate::PluginTarget::Instrument { channel })
            .is_some()
        {
            return Err(CommandError::invalid("use the hosted plugin's parameters"));
        }
        let index = self.channel_index(channel)?;
        let current = self.instrument(index)?;
        if params.kind() != current.kind() {
            return Err(other_kind(params.kind().name(), current.kind().name()));
        }
        self.set_instrument(index, checked_instrument(params)?);
        Ok("Change instrument settings")
    }

    fn add_pattern(&mut self, name: Option<String>) -> Result<Label, CommandError> {
        let count = self.project.patterns.len();
        let name = name.unwrap_or_else(|| {
            numbered_name(
                "Pattern",
                count + 1,
                self.project.patterns.iter().map(|p| p.name.as_str()),
            )
        });
        let id = PatternId(self.allocate()?);
        self.push(Edit::Pattern(ListEdit::Insert {
            index: count,
            item: Pattern {
                note_curves: Vec::new(),
                events: Vec::new(),
                time_signature: None,
                timeline: Default::default(),
                id,
                name,
                color: palette_color(count),
                length_steps: DEFAULT_PATTERN_STEPS,
                lanes: Vec::new(),
            },
        }));
        self.created.push(id.0);
        Ok("Add pattern")
    }

    fn edit_pattern_timeline(
        &mut self,
        id: PatternId,
        expected: crate::Timeline,
        expected_signature: Option<TimeSignature>,
        edit: crate::PatternTimelineEdit,
    ) -> Result<Label, CommandError> {
        let index = self.pattern_index(id)?;
        let pattern = &self.project.patterns[index];
        if pattern.timeline != expected || pattern.time_signature != expected_signature {
            return Err(CommandError::invalid(
                "the captured pattern timeline changed; reopen the editor",
            ));
        }
        let old = PatternInfo::of(pattern);
        let mut new = old.clone();
        let mut created = None;
        let label = match edit {
            crate::PatternTimelineEdit::SetSignature { signature } => {
                new.time_signature = signature;
                "Change pattern signature"
            }
            crate::PatternTimelineEdit::AddMeter { tick, signature } => {
                let allocated = self.allocate()?;
                new.timeline.meters.push(crate::MeterChange {
                    id: crate::MeterChangeId(allocated),
                    tick,
                    signature,
                });
                created = Some(allocated);
                "Add pattern meter"
            }
            crate::PatternTimelineEdit::UpdateMeter { change } => {
                let current = new
                    .timeline
                    .meters
                    .iter_mut()
                    .find(|meter| meter.id == change.id)
                    .ok_or_else(|| CommandError::invalid("the pattern meter does not exist"))?;
                *current = change;
                "Change pattern meter"
            }
            crate::PatternTimelineEdit::RemoveMeter { id } => {
                if !new.timeline.meters.iter().any(|meter| meter.id == id) {
                    return Err(CommandError::invalid("the pattern meter does not exist"));
                }
                new.timeline.meters.retain(|meter| meter.id != id);
                "Remove pattern meter"
            }
            crate::PatternTimelineEdit::AddMarker { tick, name } => {
                let allocated = self.allocate()?;
                new.timeline.markers.push(crate::TimelineMarker {
                    id: crate::TimelineMarkerId(allocated),
                    tick,
                    name: name.trim().to_owned(),
                    kind: crate::MarkerKind::Named,
                });
                created = Some(allocated);
                "Add pattern marker"
            }
            crate::PatternTimelineEdit::UpdateMarker { mut marker } => {
                marker.name = marker.name.trim().to_owned();
                let current = new
                    .timeline
                    .markers
                    .iter_mut()
                    .find(|current| current.id == marker.id)
                    .ok_or_else(|| CommandError::invalid("the pattern marker does not exist"))?;
                *current = marker;
                "Change pattern marker"
            }
            crate::PatternTimelineEdit::RemoveMarker { id } => {
                if !new.timeline.markers.iter().any(|marker| marker.id == id) {
                    return Err(CommandError::invalid("the pattern marker does not exist"));
                }
                new.timeline.markers.retain(|marker| marker.id != id);
                "Remove pattern marker"
            }
        };
        new.timeline.meters.sort_by_key(|meter| meter.tick);
        new.timeline
            .markers
            .sort_by_key(|marker| (marker.tick, marker.id));
        let candidate = Pattern {
            time_signature: new.time_signature,
            timeline: new.timeline.clone(),
            ..self.project.patterns[index].clone()
        };
        candidate
            .check_musical(self.project.settings.time_signature, self.project.next_id)
            .map_err(CommandError::invalid)?;
        self.push(Edit::PatternInfo {
            id,
            change: Change { old, new },
        });
        if let Some(id) = created {
            self.created.push(id);
        }
        Ok(label)
    }

    fn remove_pattern(&mut self, id: PatternId) -> Result<Label, CommandError> {
        let index = self.pattern_index(id)?;
        if self.project.patterns.len() == 1 {
            return Err(CommandError::invalid(
                "a project needs at least one pattern",
            ));
        }
        let clips = self.clips_where(
            |clip| matches!(clip.content, ClipContent::Pattern { pattern } if pattern == id),
        );
        self.push(Edit::Clips {
            remove: clips,
            insert: Vec::new(),
        });
        let item = self.project.patterns[index].clone();
        self.push(Edit::Pattern(ListEdit::Remove { index, item }));
        Ok("Delete pattern")
    }

    fn duplicate_pattern(&mut self, id: PatternId) -> Result<Label, CommandError> {
        let index = self.pattern_index(id)?;
        let original = &self.project.patterns[index];
        let name = copy_name(
            &original.name,
            self.project.patterns.iter().map(|p| p.name.as_str()),
        );
        let mut copy = Pattern {
            name,
            ..original.clone()
        };
        copy.id = PatternId(self.allocate()?);
        for meter in &mut copy.timeline.meters {
            meter.id = crate::MeterChangeId(self.allocate()?);
        }
        for marker in &mut copy.timeline.markers {
            marker.id = crate::TimelineMarkerId(self.allocate()?);
        }
        for lane in &mut copy.events {
            lane.id = AutomationId(self.allocate()?);
        }
        for lane in &mut copy.lanes {
            // Ids go out in lane order, so the copies sort the same way.
            for note in &mut lane.notes {
                let source = note.id;
                note.id = NoteId(self.allocate()?);
                for curve in copy
                    .note_curves
                    .iter_mut()
                    .filter(|curve| curve.note == source)
                {
                    curve.note = note.id;
                }
            }
        }
        self.created.push(copy.id.0);
        copy.note_curves
            .sort_by_key(|curve| (curve.note, curve.parameter));
        self.push(Edit::Pattern(ListEdit::Insert {
            index: index + 1,
            item: copy,
        }));
        Ok("Duplicate pattern")
    }

    fn move_pattern(&mut self, id: PatternId, index: u32) -> Result<Label, CommandError> {
        let from = self.pattern_index(id)?;
        let to = (index as usize).min(self.project.patterns.len() - 1);
        self.push(Edit::MovePattern(Move { from, to }));
        Ok("Move pattern")
    }

    fn update_pattern(
        &mut self,
        id: PatternId,
        patch: PatternPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename pattern"),
                (patch.color.is_some(), "Change pattern color"),
                (patch.length_steps.is_some(), "Change pattern length"),
            ],
            "Change pattern",
        );
        let old = PatternInfo::of(&self.project.patterns[self.pattern_index(id)?]);
        let mut new = old.clone();
        if let Some(name) = patch.name {
            new.name = name;
        }
        if let Some(color) = patch.color {
            new.color = checked_color(color)?;
        }
        if let Some(length_steps) = patch.length_steps {
            new.length_steps = length_steps.clamp(1, MAX_PATTERN_STEPS);
        }
        self.push(Edit::PatternInfo {
            id,
            change: Change { old, new },
        });
        Ok(label)
    }

    fn toggle_step(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        step: u32,
    ) -> Result<Label, CommandError> {
        let label = "Toggle step";
        let lane = self.lane(pattern, channel)?;
        if step >= MAX_PATTERN_STEPS {
            return Err(CommandError::invalid(format!(
                "step {} is past the end of the longest pattern, which has {MAX_PATTERN_STEPS} steps",
                u64::from(step) + 1
            )));
        }
        let start = step * TICKS_PER_STEP;
        let on_step = notes_where(lane, |note| note.start == start);

        if !on_step.is_empty() {
            self.push(Edit::Notes {
                pattern,
                channel,
                remove: on_step,
                insert: Vec::new(),
            });
            return Ok(label);
        }
        let id = NoteId(self.allocate()?);
        self.push(Edit::Notes {
            pattern,
            channel,
            remove: Vec::new(),
            insert: vec![Note {
                id,
                start,
                length: TICKS_PER_STEP,
                key: DEFAULT_KEY,
                velocity: DEFAULT_VELOCITY,
                pan: 0.0,
                expression: crate::NoteExpression::default(),
            }],
        });
        self.created.push(id.0);
        Ok(label)
    }

    fn add_notes(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        notes: &[NoteInit],
    ) -> Result<Label, CommandError> {
        self.lane(pattern, channel)?;
        let mut insert = Vec::with_capacity(notes.len());
        for init in notes {
            // The id is filled in below, once every note is known to be valid.
            insert.push(placed_note(Note {
                id: NoteId(0),
                start: init.start,
                length: init.length,
                key: init.key,
                velocity: init.velocity.unwrap_or(DEFAULT_VELOCITY),
                pan: init.pan.unwrap_or(0.0),
                expression: init.expression.unwrap_or_default(),
            })?);
        }
        for note in &mut insert {
            note.id = NoteId(self.allocate()?);
            self.created.push(note.id.0);
        }
        self.push(Edit::Notes {
            pattern,
            channel,
            remove: Vec::new(),
            insert,
        });
        Ok(plural(notes.len(), "Add note", "Add notes"))
    }

    #[allow(clippy::too_many_arguments)]
    fn fill_step_range(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        length_steps: u32,
        expected: &[Note],
        steps: std::ops::Range<u32>,
        replace: bool,
        notes: &[NoteInit],
    ) -> Result<Label, CommandError> {
        let index = self.pattern_index(pattern)?;
        let current = self
            .lane(pattern, channel)?
            .map(|lane| lane.notes.as_slice())
            .unwrap_or(&[]);
        if self.project.patterns[index].length_steps != length_steps || current != expected {
            return Err(CommandError::invalid(
                "the step fill preview is stale; reopen Advanced step fill",
            ));
        }
        if steps.start >= steps.end || steps.end > length_steps || length_steps > MAX_PATTERN_STEPS
        {
            return Err(CommandError::invalid(
                "the step fill range must be inside the pattern",
            ));
        }
        if notes.len() > (steps.end - steps.start) as usize {
            return Err(CommandError::invalid(
                "a step fill can contain at most one note per step",
            ));
        }
        let ticks = steps.start * TICKS_PER_STEP..steps.end * TICKS_PER_STEP;
        let mut starts = HashSet::new();
        let mut insert = Vec::with_capacity(notes.len());
        for init in notes {
            if !ticks.contains(&init.start)
                || init.start % TICKS_PER_STEP != 0
                || init.length == 0
                || init.length > TICKS_PER_STEP
                || !starts.insert(init.start)
            {
                return Err(CommandError::invalid(
                    "step fill notes need unique in-range grid starts and lengths of 1..=240 ticks",
                ));
            }
            insert.push(placed_note(Note {
                id: NoteId(0),
                start: init.start,
                length: init.length,
                key: init.key,
                velocity: init.velocity.unwrap_or(DEFAULT_VELOCITY),
                pan: init.pan.unwrap_or(0.0),
                expression: init.expression.unwrap_or_default(),
            })?);
        }
        let mut remove = Vec::new();
        for old in current {
            if !ticks.contains(&old.start) {
                continue;
            }
            if replace {
                let matching = insert
                    .iter()
                    .position(|new| Note { id: old.id, ..*new } == *old);
                if let Some(index) = matching {
                    insert.remove(index);
                } else {
                    remove.push(*old);
                }
            } else {
                insert.retain(|new| new.start != old.start);
            }
        }
        // All checks precede the first allocation/edit; execute also rolls
        // back id exhaustion, including allocations partway through this loop.
        for note in &mut insert {
            note.id = NoteId(self.allocate()?);
            self.created.push(note.id.0);
        }
        self.push(Edit::Notes {
            pattern,
            channel,
            remove,
            insert,
        });
        Ok("Advanced step fill")
    }

    fn remove_notes(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        notes: &[NoteId],
    ) -> Result<Label, CommandError> {
        let lane = self.lane(pattern, channel)?;
        let wanted: HashSet<NoteId> = notes.iter().copied().collect();
        let remove = notes_where(lane, |note| wanted.contains(&note.id));
        if remove.len() < wanted.len() {
            let found = remove.iter().map(|note| note.id);
            return Err(first_missing("note", notes.iter().copied(), found));
        }
        self.push(Edit::Notes {
            pattern,
            channel,
            remove,
            insert: Vec::new(),
        });
        Ok(plural(notes.len(), "Delete note", "Delete notes"))
    }

    fn captured_notes(
        &self,
        pattern: PatternId,
        channel: ChannelId,
        expected: &[Note],
        updates: &[NoteUpdate],
    ) -> Result<(), CommandError> {
        if expected.is_empty() || updates.is_empty() {
            return Err(CommandError::invalid(
                "a captured property edit needs notes and updates",
            ));
        }
        let lane = self
            .project
            .pattern(pattern)
            .and_then(|pattern| pattern.lane(channel))
            .ok_or_else(|| CommandError::invalid("the captured note lane no longer exists"))?;
        let current: HashMap<NoteId, &Note> =
            lane.notes.iter().map(|note| (note.id, note)).collect();
        let captured: HashMap<NoteId, &Note> =
            expected.iter().map(|note| (note.id, note)).collect();
        if captured.len() != expected.len()
            || expected
                .iter()
                .any(|note| current.get(&note.id).copied() != Some(note))
            || updates
                .iter()
                .any(|update| !captured.contains_key(&update.id))
        {
            return Err(CommandError::invalid(
                "the captured notes changed; reopen the property editor",
            ));
        }
        Ok(())
    }

    fn captured_note_sources(
        &self,
        pattern: PatternId,
        channel: ChannelId,
        expected: &[Note],
    ) -> Result<(), CommandError> {
        if expected.is_empty() {
            return Err(CommandError::invalid("choose source notes"));
        }
        let lane = self
            .project
            .pattern(pattern)
            .and_then(|pattern| pattern.lane(channel))
            .ok_or_else(|| CommandError::invalid("the captured note lane no longer exists"))?;
        let current: HashMap<NoteId, &Note> =
            lane.notes.iter().map(|note| (note.id, note)).collect();
        let ids: std::collections::HashSet<_> = expected.iter().map(|note| note.id).collect();
        if ids.len() != expected.len()
            || expected
                .iter()
                .any(|note| current.get(&note.id).copied() != Some(note))
        {
            return Err(CommandError::invalid("the captured source notes changed"));
        }
        Ok(())
    }

    fn update_notes(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        updates: &[NoteUpdate],
    ) -> Result<Label, CommandError> {
        let lane = self.lane(pattern, channel)?;
        let wanted: HashSet<NoteId> = updates.iter().map(|update| update.id).collect();
        let remove = notes_where(lane, |note| wanted.contains(&note.id));
        if remove.len() < wanted.len() {
            let wanted = updates.iter().map(|update| update.id);
            let found = remove.iter().map(|note| note.id);
            return Err(first_missing("note", wanted, found));
        }

        // Two updates may name the same note; the later one builds on the
        // earlier one.
        let mut changed: HashMap<NoteId, Note> = remove.iter().map(|n| (n.id, *n)).collect();
        let mut moves = false;
        for update in updates {
            if let Some(note) = changed.get_mut(&update.id) {
                let patch = &update.patch;
                let new = Note {
                    id: note.id,
                    start: patch.start.unwrap_or(note.start),
                    length: patch.length.unwrap_or(note.length),
                    key: patch.key.unwrap_or(note.key),
                    velocity: patch.velocity.unwrap_or(note.velocity),
                    pan: patch.pan.unwrap_or(note.pan),
                    expression: patch.expression.unwrap_or(note.expression),
                };
                // A start that moves while the end stays where it was is
                // the note being resized from its front.
                let end = |note: &Note| u64::from(note.start) + u64::from(note.length);
                let resized = patch.length.is_some() && end(&new) == end(note);
                moves |= patch.key.is_some() || (patch.start.is_some() && !resized);
                // A note that is already past the end of the longest
                // pattern, as an older file may hold, can still be changed
                // where it is. It cannot be moved further out.
                *note = if new.start == note.start {
                    checked_note(new)?
                } else {
                    placed_note(new)?
                };
            }
        }
        let label = if moves {
            plural(updates.len(), "Move note", "Move notes")
        } else {
            let has = |field: fn(&NotePatch) -> bool| updates.iter().any(|u| field(&u.patch));
            single_label(
                &[
                    (
                        has(|p| p.length.is_some()),
                        plural(updates.len(), "Resize note", "Resize notes"),
                    ),
                    (has(|p| p.velocity.is_some()), "Change note velocity"),
                    (has(|p| p.pan.is_some()), "Change note pan"),
                    (has(|p| p.expression.is_some()), "Change note expression"),
                ],
                plural(updates.len(), "Change note", "Change notes"),
            )
        };
        let (remove, insert): (Vec<Note>, Vec<Note>) = remove
            .into_iter()
            .filter_map(|old| {
                let new = changed.get(&old.id).copied()?;
                (new != old).then_some((old, new))
            })
            .unzip();
        self.push(Edit::Notes {
            pattern,
            channel,
            remove,
            insert,
        });
        Ok(label)
    }

    fn transform_notes(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        expected: &[Note],
        transform: crate::NoteTransform,
    ) -> Result<Label, CommandError> {
        let selected = crate::piano_tools::selection(expected)?;
        let lane = self.lane(pattern, channel)?;
        let wanted: HashSet<_> = selected.iter().map(|n| n.id).collect();
        let current: HashMap<_, _> = lane
            .into_iter()
            .flat_map(|l| &l.notes)
            .filter(|n| wanted.contains(&n.id))
            .map(|n| (n.id, n))
            .collect();
        for note in &selected {
            if current.get(&note.id).copied() != Some(note) {
                return Err(CommandError::invalid(
                    "the selected notes changed; reopen the tool",
                ));
            }
        }
        if let crate::NoteTransform::Quantize {
            musical: Some(grid),
            ..
        } = &transform
        {
            let source = &self.project.patterns[self.pattern_index(pattern)?];
            if grid.signature != source.effective_signature(self.project.settings.time_signature)
                || grid.meters != source.timeline.meters
            {
                return Err(CommandError::invalid(
                    "the pattern meter changed; reopen the quantize tool",
                ));
            }
        }
        let label = transform.label();
        let index = self.pattern_index(pattern)?;
        let source_curves: Vec<_> = self.project.patterns[index]
            .note_curves
            .iter()
            .filter(|curve| wanted.contains(&curve.note))
            .cloned()
            .collect();
        let crop = matches!(
            &transform,
            crate::NoteTransform::Chop { .. } | crate::NoteTransform::ChopPattern { .. }
        );
        let (mut insert, sources) =
            crate::piano_tools::transform_mapped(&selected, transform, &source_curves)?;
        // Identity transformations preserve history, dirty state and redo.
        if selected == insert {
            return Ok(label);
        }
        let original: HashMap<_, _> = selected.iter().map(|note| (note.id, *note)).collect();
        let mut by_source: HashMap<_, Vec<_>> = HashMap::new();
        for curve in &source_curves {
            by_source.entry(curve.note).or_default().push(curve);
        }
        let mut new_curves: Vec<_> = self.project.patterns[index]
            .note_curves
            .iter()
            .filter(|curve| !wanted.contains(&curve.note))
            .cloned()
            .collect();
        let mut point_count = new_curves
            .iter()
            .map(|curve| curve.points.len())
            .sum::<usize>();
        for note in &mut insert {
            let source = sources.get(&note.id).copied().unwrap_or(note.id);
            if sources.contains_key(&note.id) {
                note.id = NoteId(self.allocate()?);
                self.created.push(note.id.0);
            }
            if let Some(curves) = by_source.get(&source) {
                for curve in curves {
                    let remapped = if crop {
                        let before = original[&source];
                        let from = f64::from(note.start.saturating_sub(before.start))
                            / f64::from(before.length);
                        let to = f64::from((note.start + note.length).saturating_sub(before.start))
                            / f64::from(before.length);
                        curve.cropped(from, to, note.id)
                    } else {
                        let mut copy = (**curve).clone();
                        copy.note = note.id;
                        copy
                    };
                    point_count = point_count.saturating_add(remapped.points.len());
                    if point_count > crate::note_curves::MAX_PATTERN_CURVE_POINTS {
                        return Err(CommandError::invalid(
                            "the transform exceeds the pattern expression-curve budget",
                        ));
                    }
                    new_curves.push(remapped);
                }
            }
        }
        let end = insert.iter().map(|n| n.start + n.length).max().unwrap_or(0);
        self.push(Edit::Notes {
            pattern,
            channel,
            remove: selected,
            insert,
        });
        let index = self.pattern_index(pattern)?;
        let old = PatternInfo::of(&self.project.patterns[index]);
        let steps = end.div_ceil(TICKS_PER_STEP);
        let mut new = old.clone();
        new.length_steps = new.length_steps.max(steps);
        new_curves.sort_by_key(|curve| (curve.note, curve.parameter));
        new.note_curves = new_curves;
        let mut proposed = self.project.patterns[index].clone();
        proposed.note_curves = new.note_curves.clone();
        crate::note_curves::check(&proposed).map_err(CommandError::invalid)?;
        self.push(Edit::PatternInfo {
            id: pattern,
            change: Change { old, new },
        });
        Ok(label)
    }

    fn clear_lane(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
    ) -> Result<Label, CommandError> {
        let remove = self
            .lane(pattern, channel)?
            .map(|lane| lane.notes.clone())
            .unwrap_or_default();
        self.push(Edit::Notes {
            pattern,
            channel,
            remove,
            insert: Vec::new(),
        });
        Ok("Clear notes")
    }

    fn add_mixer_track(&mut self, name: Option<String>) -> Result<Label, CommandError> {
        let tracks = &self.project.mixer.tracks;
        let count = tracks.len();
        if tracks.iter().filter(|track| !track.current).count() >= crate::MAX_MIXER_SIGNAL_TRACKS {
            return Err(CommandError::invalid(
                "the mixer is full: it holds 500 inserts plus Master",
            ));
        }
        let name = name.unwrap_or_else(|| {
            numbered_name("Insert", count, tracks.iter().map(|t| t.name.as_str()))
        });
        // The master does not take a palette color, so inserts start the cycle.
        let color = palette_color(count.saturating_sub(1));
        let id = TrackId(self.allocate()?);
        self.push(Edit::MixerTrack(ListEdit::Insert {
            index: count,
            item: new_mixer_track(id, name, color),
        }));
        self.created.push(id.0);
        Ok("Add mixer track")
    }

    fn remove_mixer_track(&mut self, id: TrackId) -> Result<Label, CommandError> {
        let index = self.mixer_track_index(id)?;
        if id == TrackId::MASTER {
            return Err(CommandError::invalid("the master track cannot be deleted"));
        }
        self.remove_automations_of(|target| match *target {
            AutomationTarget::TrackVolume { track }
            | AutomationTarget::TrackPan { track }
            | AutomationTarget::TrackParam { track, .. }
            | AutomationTarget::EffectParam { track, .. }
            | AutomationTarget::EffectMix { track, .. } => track == id,
            AutomationTarget::SendGain { track, target }
            | AutomationTarget::SidechainGain { track, target } => track == id || target == id,
            _ => false,
        });
        let channels: Vec<Channel> = self
            .project
            .channels
            .iter()
            .filter(|channel| channel.mixer_track == id)
            .cloned()
            .collect();
        for old in channels {
            let new = Channel {
                mixer_track: TrackId::MASTER,
                ..old.clone()
            };
            self.push(Edit::SetChannel(Box::new(Change { old, new })));
        }
        let (remove, insert): (Vec<Clip>, Vec<Clip>) = self
            .project
            .playlist
            .clips
            .iter()
            .filter_map(|old| {
                let mut new = old.clone();
                match &mut new.content {
                    ClipContent::Audio { mixer_track, .. } if *mixer_track == id => {
                        *mixer_track = TrackId::MASTER;
                    }
                    _ => return None,
                }
                Some((old.clone(), new))
            })
            .unzip();
        self.push(Edit::Clips { remove, insert });
        // The master feeds no other track, so routing a track's output there
        // cannot close a loop.
        let tracks: Vec<MixerTrack> = self
            .project
            .mixer
            .tracks
            .iter()
            .filter(|track| track.id != id)
            .cloned()
            .collect();
        for old in tracks {
            let mut new = old.clone();
            if new.output == Some(id) {
                new.output = Some(TrackId::MASTER);
            }
            new.sends.retain(|send| send.target != id);
            new.sidechains.retain(|send| send.target != id);
            self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
        }
        let item = self.project.mixer.tracks[index].clone();
        self.push(Edit::MixerTrack(ListEdit::Remove { index, item }));
        Ok("Delete mixer track")
    }

    fn update_mixer_track(
        &mut self,
        id: TrackId,
        patch: MixerTrackPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename mixer track"),
                (patch.color.is_some(), "Change mixer track color"),
                (patch.volume.is_some(), "Change mixer track volume"),
                (patch.pan.is_some(), "Change mixer track pan"),
                (patch.muted == Some(true), "Mute mixer track"),
                (patch.muted == Some(false), "Unmute mixer track"),
                (patch.solo == Some(true), "Solo mixer track"),
                (patch.solo == Some(false), "Unsolo mixer track"),
                (patch.recording.is_some(), "Change mixer recording route"),
                (patch.latency_offset_ms.is_some(), "Correct mixer latency"),
                (patch.processing.is_some(), "Change track EQ and stereo"),
                (patch.dock.is_some(), "Dock mixer track"),
            ],
            "Change mixer track",
        );
        let old = self.project.mixer.tracks[self.mixer_track_index(id)?].clone();
        if old.current && (patch.solo == Some(true) || patch.recording.is_some()) {
            return Err(CommandError::invalid(
                "Current has no solo or recording input",
            ));
        }
        let mut new = old.clone();
        if let Some(offset) = patch.latency_offset_ms {
            if !offset.is_finite() || offset.abs() > 1000.0 {
                return Err(CommandError::invalid(
                    "Mixer latency correction must be -1000 to 1000 ms",
                ));
            }
            new.latency_offset_ms = offset;
        }
        if let Some(dock) = patch.dock {
            new.dock = dock;
        }
        if let Some(processing) = patch.processing {
            use windfall_dsp::ParamSet;
            if processing.sanitized() != processing {
                return Err(CommandError::invalid(
                    "Track processing settings are outside their ranges",
                ));
            }
            new.processing = processing;
        }
        if let Some(name) = patch.name {
            new.name = name;
        }
        if let Some(color) = patch.color {
            new.color = checked_color(color)?;
        }
        if let Some(volume) = patch.volume {
            new.volume = clamped("the volume", volume, 0.0, MAX_GAIN)?;
        }
        if let Some(pan) = patch.pan {
            new.pan = clamped("the pan", pan, -1.0, 1.0)?;
        }
        if let Some(muted) = patch.muted {
            new.muted = muted;
        }
        if let Some(solo) = patch.solo {
            new.solo = solo;
        }
        if let Some(recording) = patch.recording {
            recording.check().map_err(CommandError::invalid)?;
            new.recording = (recording != crate::MixerRecording::default()).then_some(recording);
        }
        self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
        Ok(label)
    }

    fn set_track_output(
        &mut self,
        id: TrackId,
        output: Option<TrackId>,
    ) -> Result<Label, CommandError> {
        let old = self.project.mixer.tracks[self.signal_track_index(id)?].clone();
        if id == TrackId::MASTER {
            return Err(CommandError::invalid(
                "the master track's output cannot be changed",
            ));
        }
        if let Some(output) = output {
            self.signal_track_index(output)?;
            if reaches(&self.project.mixer, output, id) {
                return Err(CommandError::invalid(
                    "that output would make the mixer routing loop back on itself",
                ));
            }
        }
        let new = MixerTrack {
            output,
            ..old.clone()
        };
        self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
        Ok("Route mixer track")
    }

    fn set_send(
        &mut self,
        from: TrackId,
        to: TrackId,
        gain: Option<f32>,
    ) -> Result<Label, CommandError> {
        let old = self.project.mixer.tracks[self.signal_track_index(from)?].clone();
        self.signal_track_index(to)?;
        let mut new = old.clone();
        let existing = new.sends.iter().position(|send| send.target == to);
        let label = match (gain, existing) {
            (None, Some(index)) => {
                new.sends.remove(index);
                self.remove_automations_of(|target| {
                    *target
                        == AutomationTarget::SendGain {
                            track: from,
                            target: to,
                        }
                });
                "Remove send"
            }
            (None, None) => "Remove send",
            (Some(gain), existing) => {
                let gain = clamped("the send level", gain, 0.0, MAX_GAIN)?;
                if from == TrackId::MASTER {
                    return Err(CommandError::invalid(
                        "the master track cannot send to other tracks",
                    ));
                }
                match existing {
                    Some(index) => {
                        new.sends[index].gain = gain;
                        "Change send level"
                    }
                    None => {
                        if reaches(&self.project.mixer, to, from) {
                            return Err(CommandError::invalid(
                                "that send would make the mixer routing loop back on itself",
                            ));
                        }
                        new.sends.push(Send { target: to, gain });
                        "Add send"
                    }
                }
            }
        };
        self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
        Ok(label)
    }

    fn add_effect(
        &mut self,
        track: TrackId,
        kind: EffectKind,
        index: Option<u32>,
    ) -> Result<Label, CommandError> {
        let track = self.mixer_track_index(track)?;
        let count = self.room_for_effect(track)?;
        let index = index.map_or(count, |index| (index as usize).min(count));
        let id = EffectId(self.allocate()?);
        let slot = EffectSlot {
            id,
            enabled: true,
            mix: 1.0,
            params: kind.default_params(),
        };
        self.change_track(track, |track| track.effects.insert(index, slot));
        self.created.push(id.0);
        Ok("Add effect")
    }

    fn remove_effect(&mut self, track: TrackId, effect: EffectId) -> Result<Label, CommandError> {
        let (track, slot) = self.effect_index(track, effect)?;
        self.remove_automations_of(|target| targets_effect(target, effect));
        self.change_track(track, |track| {
            track.effects.remove(slot);
        });
        Ok("Delete effect")
    }

    fn move_effect(
        &mut self,
        track: TrackId,
        effect: EffectId,
        to_track: Option<TrackId>,
        index: u32,
    ) -> Result<Label, CommandError> {
        let (from_track, from) = self.effect_index(track, effect)?;
        let to_track = match to_track {
            Some(id) => self.mixer_track_index(id)?,
            None => from_track,
        };
        let index = index as usize;
        if to_track == from_track {
            self.change_track(from_track, |track| {
                let slot = track.effects.remove(from);
                let to = index.min(track.effects.len());
                track.effects.insert(to, slot);
            });
        } else {
            let count = self.room_for_effect(to_track)?;
            let slot = self.project.mixer.tracks[from_track].effects[from].clone();
            self.change_track(from_track, |track| {
                track.effects.remove(from);
            });
            self.change_track(to_track, |track| {
                track.effects.insert(index.min(count), slot);
            });
            // The automations of the effect name the track it is on.
            let home = self.project.mixer.tracks[to_track].id;
            let moved: Vec<Automation> = self
                .project
                .automations
                .iter()
                .filter(|automation| targets_effect(&automation.target, effect))
                .cloned()
                .collect();
            for old in moved {
                let mut new = old.clone();
                match &mut new.target {
                    AutomationTarget::EffectParam { track, .. }
                    | AutomationTarget::EffectMix { track, .. } => *track = home,
                    _ => {}
                }
                self.push(Edit::SetAutomation(Box::new(Change { old, new })));
            }
            self.map_pattern_events(|lanes| {
                let mut lanes = lanes.to_vec();
                for lane in &mut lanes {
                    if !targets_effect(&lane.target, effect) {
                        continue;
                    }
                    match &mut lane.target {
                        AutomationTarget::EffectParam { track, .. }
                        | AutomationTarget::EffectMix { track, .. } => *track = home,
                        _ => {}
                    }
                }
                lanes
            });
        }
        Ok("Move effect")
    }

    /// Replaces the event lane a pattern has for `target`, as
    /// [`Command::SetPatternEvents`] describes.
    fn set_pattern_events(
        &mut self,
        pattern: PatternId,
        target: AutomationTarget,
        expected: Vec<AutomationPoint>,
        points: Vec<AutomationPoint>,
    ) -> Result<Label, CommandError> {
        let index = self.pattern_index(pattern)?;
        let before = &self.project.patterns[index];
        if crate::pattern_events::captured(before, &target) != expected {
            return Err(CommandError::invalid(
                "the captured event automation changed; reopen the event editor",
            ));
        }
        let exists = before.event_lane(&target).is_some();
        let points = crate::pattern_events::checked_points(points)?;
        // What a lane moves has to be there. Taking a lane out needs nothing.
        if !points.is_empty() && target != AutomationTarget::Tempo {
            self.automation_target(&target)?;
        }
        let label = match (exists, points.is_empty()) {
            (_, true) => "Clear event automation",
            (false, false) => "Add event automation",
            (true, false) => "Change event automation",
        };
        let fresh = if exists || points.is_empty() {
            None
        } else {
            Some(AutomationId(self.allocate()?))
        };
        let old = PatternInfo::of(&self.project.patterns[index]);
        let mut new = old.clone();
        new.events = crate::pattern_events::replaced(&old.events, target, points, fresh)?;
        if let Some(id) = fresh {
            self.created.push(id.0);
        }
        self.push(Edit::PatternInfo {
            id: pattern,
            change: Change { old, new },
        });
        Ok(label)
    }

    /// Rewrites the event lanes of every pattern that has some.
    fn map_pattern_events(
        &mut self,
        map: impl Fn(&[crate::PatternEventLane]) -> Vec<crate::PatternEventLane>,
    ) {
        let edits: Vec<Edit> = self
            .project
            .patterns
            .iter()
            .filter(|pattern| !pattern.events.is_empty())
            .filter_map(|pattern| {
                let events = map(&pattern.events);
                if events == pattern.events {
                    return None;
                }
                let old = PatternInfo::of(pattern);
                let mut new = old.clone();
                new.events = events;
                Some(Edit::PatternInfo {
                    id: pattern.id,
                    change: Change { old, new },
                })
            })
            .collect();
        for edit in edits {
            self.push(edit);
        }
    }

    fn update_effect(
        &mut self,
        track: TrackId,
        effect: EffectId,
        patch: EffectSlotPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.enabled == Some(true), "Switch effect on"),
                (patch.enabled == Some(false), "Switch effect off"),
                (patch.mix.is_some(), "Change effect mix"),
            ],
            "Change effect",
        );
        let (track, slot) = self.effect_index(track, effect)?;
        let mix = match patch.mix {
            Some(mix) => Some(clamped("the mix", mix, 0.0, 1.0)?),
            None => None,
        };
        self.change_track(track, |track| {
            let slot = &mut track.effects[slot];
            slot.enabled = patch.enabled.unwrap_or(slot.enabled);
            slot.mix = mix.unwrap_or(slot.mix);
        });
        Ok(label)
    }

    fn set_effect_param(
        &mut self,
        track: TrackId,
        effect: EffectId,
        param: u32,
        value: f32,
    ) -> Result<String, CommandError> {
        let target = crate::PluginTarget::Effect { effect };
        if let Some(plugin) = self.project.plugin(target) {
            self.effect_index(track, effect)?;
            let parameter = plugin
                .parameters
                .get(param as usize)
                .ok_or_else(|| CommandError::invalid("the plugin parameter does not exist"))?;
            let id = parameter.id;
            let label = format!("Change {}", parameter.name);
            self.plugin_param(target, id, value)?;
            return Ok(label);
        }
        let (track, slot) = self.effect_index(track, effect)?;
        let mut params = self.project.mixer.tracks[track].effects[slot].params;
        let kind = params.kind();
        if !params.set(param as usize, checked_setting(value)?) {
            return Err(no_setting(kind.name(), param));
        }
        self.change_track(track, |track| track.effects[slot].params = params);
        Ok(setting_label(kind.descriptors(), param))
    }

    fn set_effect_params(
        &mut self,
        track: TrackId,
        effect: EffectId,
        params: EffectParams,
    ) -> Result<Label, CommandError> {
        if self
            .project
            .plugin(crate::PluginTarget::Effect { effect })
            .is_some()
        {
            return Err(CommandError::invalid("use the hosted plugin's parameters"));
        }
        let (track, slot) = self.effect_index(track, effect)?;
        let kind = self.project.mixer.tracks[track].effects[slot].kind();
        if params.kind() != kind {
            return Err(other_kind(params.kind().name(), kind.name()));
        }
        let params = checked_effect(params)?;
        self.change_track(track, |track| track.effects[slot].params = params);
        Ok("Change effect settings")
    }

    fn duplicate_effect(
        &mut self,
        track: TrackId,
        effect: EffectId,
    ) -> Result<Label, CommandError> {
        let (track, slot) = self.effect_index(track, effect)?;
        self.room_for_effect(track)?;
        let mut copy = self.project.mixer.tracks[track].effects[slot].clone();
        copy.id = EffectId(self.allocate()?);
        let id = copy.id;
        self.change_track(track, |track| track.effects.insert(slot + 1, copy));
        self.created.push(id.0);
        if let Some(mut plugin) = self
            .project
            .plugin(crate::PluginTarget::Effect { effect })
            .cloned()
        {
            plugin.target = crate::PluginTarget::Effect { effect: id };
            self.bind_plugin(plugin);
        }
        Ok("Duplicate effect")
    }

    fn replace_effect(
        &mut self,
        track: TrackId,
        effect: EffectId,
        kind: EffectKind,
    ) -> Result<Label, CommandError> {
        let (track, slot) = self.effect_index(track, effect)?;
        let id = EffectId(self.allocate()?);
        let new = EffectSlot {
            id,
            enabled: true,
            mix: 1.0,
            params: kind.default_params(),
        };
        self.remove_automations_of(|target| targets_effect(target, effect));
        self.change_track(track, |track| track.effects[slot] = new);
        self.created.push(id.0);
        Ok("Replace effect")
    }

    fn add_playlist_track(
        &mut self,
        name: Option<String>,
        index: Option<u32>,
    ) -> Result<Label, CommandError> {
        let tracks = &self.project.playlist.tracks;
        let count = tracks.len();
        let name = name.unwrap_or_else(|| {
            numbered_name("Track", count + 1, tracks.iter().map(|t| t.name.as_str()))
        });
        let index = index.map_or(count, |index| (index as usize).min(count));
        let id = PlaylistTrackId(self.allocate()?);
        self.push(Edit::PlaylistTrack(ListEdit::Insert {
            index,
            item: PlaylistTrack {
                id,
                name,
                muted: false,
                solo: false,
                color: 0,
                height: 0,
            },
        }));
        self.enroll_active_arrangement(&[], &[id]);
        self.created.push(id.0);
        Ok("Add playlist track")
    }

    fn remove_playlist_track(&mut self, id: PlaylistTrackId) -> Result<Label, CommandError> {
        let index = self.playlist_track_index(id)?;
        let clips = self.clips_where(|clip| clip.track == id);
        self.push(Edit::Clips {
            remove: clips,
            insert: Vec::new(),
        });
        let item = self.project.playlist.tracks[index].clone();
        self.push(Edit::PlaylistTrack(ListEdit::Remove { index, item }));
        Ok("Delete playlist track")
    }

    fn update_playlist_track(
        &mut self,
        id: PlaylistTrackId,
        patch: PlaylistTrackPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename playlist track"),
                (patch.muted == Some(true), "Mute playlist track"),
                (patch.muted == Some(false), "Unmute playlist track"),
                (patch.solo == Some(true), "Solo playlist track"),
                (patch.solo == Some(false), "Unsolo playlist track"),
                (patch.color.is_some(), "Color playlist track"),
                (patch.height.is_some(), "Resize playlist track"),
            ],
            "Change playlist track",
        );
        let old = self.project.playlist.tracks[self.playlist_track_index(id)?].clone();
        let mut new = old.clone();
        if let Some(name) = patch.name {
            new.name = name;
        }
        if let Some(muted) = patch.muted {
            new.muted = muted;
        }
        if let Some(solo) = patch.solo {
            new.solo = solo;
        }
        if let Some(color) = patch.color {
            new.color = color;
        }
        if let Some(height) = patch.height {
            new.height = if height == 0 {
                0
            } else {
                height.clamp(18, 160)
            };
        }
        self.push(Edit::SetPlaylistTrack(Change { old, new }));
        Ok(label)
    }

    fn move_playlist_track(
        &mut self,
        id: PlaylistTrackId,
        index: u32,
    ) -> Result<Label, CommandError> {
        let from = self.playlist_track_index(id)?;
        let to = (index as usize).min(self.project.playlist.tracks.len() - 1);
        self.push(Edit::MovePlaylistTrack(Move { from, to }));
        Ok("Move playlist track")
    }

    fn add_clips(&mut self, clips: &[ClipInit]) -> Result<Label, CommandError> {
        let mut insert = Vec::with_capacity(clips.len());
        for init in clips {
            self.playlist_track_index(init.track)?;
            let (content, natural) = self.checked_content(&init.content)?;
            let length = init.length.or(natural).ok_or_else(|| {
                CommandError::invalid(
                    "an audio clip has to be given a length: the project does not know how long its audio lasts",
                )
            })?;
            // The id is filled in below, once every clip is known to be valid.
            insert.push(checked_clip(Clip {
                id: ClipId(0),
                track: init.track,
                start: init.start,
                length,
                offset: init.offset.unwrap_or(0),
                muted: init.muted.unwrap_or(false),
                content,
            })?);
        }
        for clip in &mut insert {
            clip.id = ClipId(self.allocate()?);
            self.created.push(clip.id.0);
        }
        let ids: Vec<_> = insert.iter().map(|clip| clip.id).collect();
        let tracks: Vec<_> = insert.iter().map(|clip| clip.track).collect();
        self.push(Edit::Clips {
            remove: Vec::new(),
            insert,
        });
        self.enroll_active_arrangement(&ids, &tracks);
        Ok(plural(clips.len(), "Add clip", "Add clips"))
    }

    fn remove_clips(&mut self, clips: &[ClipId]) -> Result<Label, CommandError> {
        let wanted: HashSet<ClipId> = clips.iter().copied().collect();
        let remove = self.clips_where(|clip| wanted.contains(&clip.id));
        if remove.len() < wanted.len() {
            let found = remove.iter().map(|clip| clip.id);
            return Err(first_missing("clip", clips.iter().copied(), found));
        }
        self.push(Edit::Clips {
            remove,
            insert: Vec::new(),
        });
        Ok(plural(clips.len(), "Delete clip", "Delete clips"))
    }

    fn update_clips(&mut self, updates: &[ClipUpdate]) -> Result<Label, CommandError> {
        let has = |field: fn(&ClipPatch) -> bool| updates.iter().any(|u| field(&u.patch));
        let count = updates.len();
        let label = if has(|p| p.track.is_some() || p.start.is_some()) {
            plural(count, "Move clip", "Move clips")
        } else if has(|p| p.length.is_some() || p.offset.is_some()) {
            plural(count, "Resize clip", "Resize clips")
        } else if has(|p| p.muted == Some(true)) {
            plural(count, "Mute clip", "Mute clips")
        } else if has(|p| p.muted == Some(false)) {
            plural(count, "Unmute clip", "Unmute clips")
        } else {
            plural(count, "Change clip", "Change clips")
        };

        let wanted: HashSet<ClipId> = updates.iter().map(|update| update.id).collect();
        let remove = self.clips_where(|clip| wanted.contains(&clip.id));
        if remove.len() < wanted.len() {
            let wanted = updates.iter().map(|update| update.id);
            let found = remove.iter().map(|clip| clip.id);
            return Err(first_missing("clip", wanted, found));
        }

        // Two updates may name the same clip; the later one builds on the
        // earlier one.
        let mut changed: HashMap<ClipId, Clip> = remove.iter().map(|c| (c.id, c.clone())).collect();
        for update in updates {
            if let Some(clip) = changed.get_mut(&update.id) {
                let patch = &update.patch;
                if let Some(track) = patch.track {
                    self.playlist_track_index(track)?;
                    clip.track = track;
                }
                clip.start = patch.start.unwrap_or(clip.start);
                clip.length = patch.length.unwrap_or(clip.length);
                clip.offset = patch.offset.unwrap_or(clip.offset);
                clip.muted = patch.muted.unwrap_or(clip.muted);
                *clip = checked_clip(clip.clone())?;
            }
        }
        let (remove, insert): (Vec<Clip>, Vec<Clip>) = remove
            .into_iter()
            .filter_map(|old| {
                let new = changed.remove(&old.id)?;
                (new != old).then_some((old, new))
            })
            .unzip();
        self.push(Edit::Clips { remove, insert });
        Ok(label)
    }

    fn update_audio_clips(&mut self, updates: &[AudioClipUpdate]) -> Result<Label, CommandError> {
        let has = |field: fn(&AudioClipPatch) -> bool| updates.iter().any(|u| field(&u.patch));
        let label = single_label(
            &[
                (
                    has(|p| p.mixer_track.is_some() || p.output.is_some()),
                    "Route clip",
                ),
                (has(|p| p.gain.is_some()), "Change clip gain"),
                (has(|p| p.normalize == Some(true)), "Normalize audio clip"),
                (
                    has(|p| p.normalize == Some(false)),
                    "Clear audio clip normalize",
                ),
                (has(|p| p.pan.is_some()), "Change clip pan"),
                (
                    has(|p| p.fade_in.is_some() || p.fade_out.is_some()),
                    "Change clip fade",
                ),
                (has(|p| p.reverse.is_some()), "Reverse clip"),
                (has(|p| p.pitch.is_some()), "Change clip pitch"),
                (has(|p| p.stretch.is_some()), "Change clip stretch"),
            ],
            plural(updates.len(), "Change audio clip", "Change audio clips"),
        );

        let wanted: HashSet<ClipId> = updates.iter().map(|update| update.id).collect();
        let remove = self.clips_where(|clip| wanted.contains(&clip.id));
        if remove.len() < wanted.len() {
            let wanted = updates.iter().map(|update| update.id);
            let found = remove.iter().map(|clip| clip.id);
            return Err(first_missing("clip", wanted, found));
        }

        // Two updates may name the same clip; the later one builds on the
        // earlier one.
        let mut changed: HashMap<ClipId, Clip> = remove.iter().map(|c| (c.id, c.clone())).collect();
        for update in updates {
            let Some(clip) = changed.get_mut(&update.id) else {
                continue;
            };
            let ClipContent::Audio {
                sample,
                mixer_track,
                output,
                normalize,
                gain,
                pan,
                fade_in,
                fade_out,
                reverse,
                pitch,
                stretch,
            } = clip.content
            else {
                return Err(CommandError::invalid(format!(
                    "clip {} is not an audio clip",
                    clip.id.0
                )));
            };
            let patch = &update.patch;
            let patched = ClipContent::Audio {
                sample,
                mixer_track: patch.mixer_track.unwrap_or(mixer_track),
                output: patch.output.unwrap_or(output),
                normalize: patch.normalize.unwrap_or(normalize),
                gain: patch.gain.unwrap_or(gain),
                pan: patch.pan.unwrap_or(pan),
                fade_in: patch.fade_in.unwrap_or(fade_in),
                fade_out: patch.fade_out.unwrap_or(fade_out),
                reverse: patch.reverse.unwrap_or(reverse),
                pitch: patch.pitch.unwrap_or(pitch),
                stretch: patch.stretch.unwrap_or(stretch),
            };
            clip.content = self.checked_content(&patched)?.0;
        }
        let (remove, insert): (Vec<Clip>, Vec<Clip>) = remove
            .into_iter()
            .filter_map(|old| {
                let new = changed.remove(&old.id)?;
                (new != old).then_some((old, new))
            })
            .unzip();
        self.push(Edit::Clips { remove, insert });
        Ok(label)
    }

    /// What a clip is to play, checked against the project and with every
    /// value inside its range, and the length such a clip has when none is
    /// given, if the project can tell.
    fn checked_content(
        &self,
        content: &ClipContent,
    ) -> Result<(ClipContent, Option<u32>), CommandError> {
        match *content {
            ClipContent::Pattern { pattern } => {
                let pattern = &self.project.patterns[self.pattern_index(pattern)?];
                Ok((content.clone(), Some(pattern.length_ticks())))
            }
            ClipContent::Audio {
                sample,
                mixer_track,
                output,
                normalize,
                gain,
                pan,
                fade_in,
                fade_out,
                reverse,
                pitch,
                stretch,
            } => {
                self.sample_index(sample)?;
                self.signal_track_index(mixer_track)?;
                let range = if matches!(stretch, crate::ClipStretch::Spectral { .. }) {
                    24.0
                } else {
                    MAX_TUNE_SEMITONES
                };
                if let crate::ClipStretch::Spectral { ratio, .. } = stretch
                    && (!ratio.is_finite() || !(0.25..=4.0).contains(&ratio))
                {
                    return Err(CommandError::invalid(
                        "Stretch ratio must be between 0.25 and 4",
                    ));
                }
                let checked = ClipContent::Audio {
                    sample,
                    mixer_track,
                    output,
                    normalize,
                    gain: clamped("the clip gain", gain, 0.0, MAX_GAIN)?,
                    pan: clamped("the pan", pan, -1.0, 1.0)?,
                    fade_in: fade_in.min(MAX_SONG_TICKS),
                    fade_out: fade_out.min(MAX_SONG_TICKS),
                    reverse,
                    pitch: clamped("the pitch", pitch, -range, range)?,

                    stretch,
                };
                Ok((checked, None))
            }
            ClipContent::Automation { automation } => {
                let automation = &self.project.automations[self.automation_index(automation)?];
                // The whole curve, and at least a bar to take hold of.
                let last = automation.points.last().map_or(0, |point| point.tick);
                let bar = self.project.settings.time_signature.ticks_per_bar();
                Ok((content.clone(), Some(last.max(bar))))
            }
        }
    }

    fn add_automation(
        &mut self,
        name: Option<String>,
        target: AutomationTarget,
        points: Option<Vec<AutomationPoint>>,
    ) -> Result<Label, CommandError> {
        let (range, stored) = self.automation_target(&target)?;
        let points = match points {
            Some(points) => checked_points(points)?,
            None => vec![AutomationPoint {
                tick: 0,
                value: range.normalized(stored),
                curve: 0.0,
                hold: false,
            }],
        };
        let name = name.unwrap_or_else(|| self.automation_name(&target));
        let count = self.project.automations.len();
        let id = AutomationId(self.allocate()?);
        self.push(Edit::Automation(ListEdit::Insert {
            index: count,
            item: Automation {
                id,
                name,
                color: palette_color(count),
                target,
                points,
            },
        }));
        self.created.push(id.0);
        Ok("Add automation")
    }

    fn remove_automation(&mut self, id: AutomationId) -> Result<Label, CommandError> {
        self.automation_index(id)?;
        self.take_out_automations(&[id]);
        Ok("Delete automation")
    }

    fn update_automation(
        &mut self,
        id: AutomationId,
        patch: AutomationPatch,
    ) -> Result<Label, CommandError> {
        let label = single_label(
            &[
                (patch.name.is_some(), "Rename automation"),
                (patch.color.is_some(), "Change automation color"),
            ],
            "Change automation",
        );
        let old = self.project.automations[self.automation_index(id)?].clone();
        let mut new = old.clone();
        if let Some(name) = patch.name {
            new.name = name;
        }
        if let Some(color) = patch.color {
            new.color = checked_color(color)?;
        }
        self.push(Edit::SetAutomation(Box::new(Change { old, new })));
        Ok(label)
    }

    fn set_automation_points(
        &mut self,
        id: AutomationId,
        points: Vec<AutomationPoint>,
    ) -> Result<Label, CommandError> {
        let old = self.project.automations[self.automation_index(id)?].clone();
        let new = Automation {
            points: checked_points(points)?,
            ..old.clone()
        };
        self.push(Edit::SetAutomation(Box::new(Change { old, new })));
        Ok("Change automation curve")
    }

    fn duplicate_automation(&mut self, id: AutomationId) -> Result<Label, CommandError> {
        let index = self.automation_index(id)?;
        let original = &self.project.automations[index];
        let name = copy_name(
            &original.name,
            self.project.automations.iter().map(|a| a.name.as_str()),
        );
        let mut copy = Automation {
            name,
            ..original.clone()
        };
        copy.id = AutomationId(self.allocate()?);
        self.created.push(copy.id.0);
        self.push(Edit::Automation(ListEdit::Insert {
            index: index + 1,
            item: copy,
        }));
        Ok("Duplicate automation")
    }

    /// Removes every automation whose target `gone` picks out, with its
    /// clips: what those automations move is about to leave the project.
    fn remove_automations_of(&mut self, gone: impl Fn(&AutomationTarget) -> bool) {
        self.map_pattern_events(|lanes| crate::pattern_events::without(lanes, &gone));
        // Controller links to the same targets leave in the same edit.
        let links = &self.project.settings.controller_links;
        if let Some(kept) = crate::controller_links::without_targets(links, &gone) {
            let old = self.project.settings.clone();
            let mut new = old.clone();
            new.controller_links = kept;
            self.push(Edit::Settings(Change { old, new }));
        }
        let automations = self.project.automations.iter();
        let ids: Vec<AutomationId> = automations
            .filter(|automation| gone(&automation.target))
            .map(|automation| automation.id)
            .collect();
        self.take_out_automations(&ids);
    }

    /// Removes automations and the clips that show them.
    fn take_out_automations(&mut self, ids: &[AutomationId]) {
        if ids.is_empty() {
            return;
        }
        let clips = self.clips_where(|clip| {
            let shown = clip.content.automation();
            shown.is_some_and(|automation| ids.contains(&automation))
        });
        self.push(Edit::Clips {
            remove: clips,
            insert: Vec::new(),
        });
        for id in ids {
            let automations = &self.project.automations;
            if let Some(index) = automations.iter().position(|a| a.id == *id) {
                let item = automations[index].clone();
                self.push(Edit::Automation(ListEdit::Remove { index, item }));
            }
        }
    }

    /// The range of what `target` moves and the value it has now. Fails,
    /// saying what is missing, when the project has no such thing.
    fn automation_target(
        &self,
        target: &AutomationTarget,
    ) -> Result<(AutomationRange, f32), CommandError> {
        match *target {
            AutomationTarget::ChannelVolume { channel }
            | AutomationTarget::ChannelPan { channel } => {
                self.channel_index(channel)?;
            }
            AutomationTarget::TrackVolume { track } | AutomationTarget::TrackPan { track } => {
                self.mixer_track_index(track)?;
            }
            AutomationTarget::TrackParam { track, param } => {
                use windfall_dsp::ParamSet;
                self.mixer_track_index(track)?;
                if windfall_dsp::TrackParams::descriptors()
                    .get(param as usize)
                    .is_none()
                {
                    return Err(no_setting("Track EQ and stereo", param));
                }
            }
            AutomationTarget::SidechainGain { track, target } => {
                let from = &self.project.mixer.tracks[self.mixer_track_index(track)?];
                let to = &self.project.mixer.tracks[self.mixer_track_index(target)?];
                if !from.sidechains.iter().any(|send| send.target == target) {
                    return Err(CommandError::invalid(format!(
                        "the mixer track \"{}\" has no sidechain to \"{}\"",
                        from.name, to.name
                    )));
                }
            }
            AutomationTarget::SendGain { track, target } => {
                let from = &self.project.mixer.tracks[self.mixer_track_index(track)?];
                let to = &self.project.mixer.tracks[self.mixer_track_index(target)?];
                if !from.sends.iter().any(|send| send.target == target) {
                    return Err(CommandError::invalid(format!(
                        "the mixer track \"{}\" has no send to \"{}\"",
                        from.name, to.name
                    )));
                }
            }
            AutomationTarget::EffectParam {
                track,
                effect,
                param,
            } => {
                let (track, slot) = self.effect_index(track, effect)?;
                let kind = self.project.mixer.tracks[track].effects[slot].kind();
                if kind.descriptors().get(param as usize).is_none() {
                    return Err(no_setting(kind.name(), param));
                }
            }
            AutomationTarget::EffectMix { track, effect } => {
                self.effect_index(track, effect)?;
            }
            AutomationTarget::InstrumentParam { channel, param } => {
                let kind = self.instrument(self.channel_index(channel)?)?.kind();
                if kind.descriptors().get(param as usize).is_none() {
                    return Err(no_setting(kind.name(), param));
                }
            }
            AutomationTarget::Tempo => {}
        }
        let range = self.project.automation_range(target);
        let stored = self.project.automation_stored_value(target);
        range.zip(stored).ok_or_else(|| {
            CommandError::invalid("the project has nothing for the automation to move")
        })
    }

    /// The name an automation of `target` gets when none is given. The
    /// target is known to exist.
    ///
    /// The name says what kind of thing is moved, so that a channel and
    /// the mixer track named after it do not give two automations one
    /// name: "Kick volume" is the channel's and "Kick track volume" the
    /// track's. An effect is named with its track, "Kick Reverb mix", and
    /// counted when the track has several of its kind, "Kick Reverb 2
    /// Decay". Whatever comes out is numbered if an automation has that
    /// name already: "Kick pan", then "Kick pan 2".
    fn automation_name(&self, target: &AutomationTarget) -> String {
        let project = &*self.project;
        let channel = |id| {
            project
                .channel(id)
                .map_or("", |channel| channel.name.as_str())
        };
        let track = |id| {
            project
                .mixer
                .track(id)
                .map_or("", |track| track.name.as_str())
        };
        // "Kick Reverb", or "Kick Reverb 2" for the second reverb in the
        // track's chain.
        let effect = |track_id: TrackId, effect: EffectId| {
            if let Some(plugin) = project.plugin(crate::PluginTarget::Effect { effect }) {
                return format!("{} {}", track(track_id), plugin.name);
            }
            let chain = project.mixer.track(track_id).map(|track| &track.effects);
            let chain = chain.map_or(&[][..], Vec::as_slice);
            let Some(place) = chain.iter().position(|slot| slot.id == effect) else {
                return format!("{} Effect", track(track_id));
            };
            let kind = chain[place].kind();
            let earlier = chain[..place].iter().filter(|slot| slot.kind() == kind);
            match earlier.count() {
                0 => format!("{} {}", track(track_id), kind.name()),
                earlier => format!("{} {} {}", track(track_id), kind.name(), earlier + 1),
            }
        };
        let effect_kind = |track: TrackId, effect| {
            let slot = project
                .mixer
                .track(track)
                .and_then(|track| track.effect(effect));
            slot.map(EffectSlot::kind)
        };
        let setting = |descriptors: &[ParamInfo], param: u32| {
            descriptors
                .get(param as usize)
                .map_or("setting", |info| info.name)
        };
        let name = match *target {
            AutomationTarget::ChannelVolume { channel: id } => format!("{} volume", channel(id)),
            AutomationTarget::ChannelPan { channel: id } => format!("{} pan", channel(id)),
            AutomationTarget::TrackVolume { track: id } => format!("{} track volume", track(id)),
            AutomationTarget::TrackPan { track: id } => format!("{} track pan", track(id)),
            AutomationTarget::TrackParam { track: id, param } => {
                use windfall_dsp::ParamSet;
                format!(
                    "{} {}",
                    track(id),
                    setting(windfall_dsp::TrackParams::descriptors(), param)
                )
            }
            AutomationTarget::SidechainGain {
                track: from,
                target,
            } => {
                format!("{} to {} sidechain", track(from), track(target))
            }
            AutomationTarget::SendGain {
                track: from,
                target,
            } => {
                format!("{} to {} send", track(from), track(target))
            }
            AutomationTarget::EffectParam {
                track,
                effect: id,
                param,
            } => {
                if let Some(plugin) = project.plugin(crate::PluginTarget::Effect { effect: id }) {
                    let name = plugin
                        .parameters
                        .get(param as usize)
                        .map_or("setting", |parameter| parameter.name.as_str());
                    return unused_name(
                        format!("{} {name}", effect(track, id)),
                        project
                            .automations
                            .iter()
                            .map(|automation| automation.name.as_str()),
                    );
                }
                let descriptors = effect_kind(track, id).map_or(&[][..], EffectKind::descriptors);
                format!("{} {}", effect(track, id), setting(descriptors, param))
            }
            AutomationTarget::EffectMix { track, effect: id } => {
                format!("{} mix", effect(track, id))
            }
            AutomationTarget::InstrumentParam { channel: id, param } => {
                if let Some(plugin) =
                    project.plugin(crate::PluginTarget::Instrument { channel: id })
                {
                    let name = plugin
                        .parameters
                        .get(param as usize)
                        .map_or("setting", |parameter| parameter.name.as_str());
                    return unused_name(
                        format!("{} {name}", channel(id)),
                        project
                            .automations
                            .iter()
                            .map(|automation| automation.name.as_str()),
                    );
                }
                let source = project.channel(id).map(|channel| &channel.source);
                let name = match source {
                    Some(ChannelSource::Instrument { params }) => {
                        setting(params.kind().descriptors(), param)
                    }
                    _ => "setting",
                };
                format!("{} {name}", channel(id))
            }
            AutomationTarget::Tempo => "Tempo".to_owned(),
        };
        let taken = project.automations.iter().map(|a| a.name.as_str());
        unused_name(name, taken)
    }

    fn automation_index(&self, id: AutomationId) -> Result<usize, CommandError> {
        let found = self.project.automations.iter().position(|a| a.id == id);
        found.ok_or_else(|| CommandError::not_found("automation", id))
    }

    fn sample_index(&self, id: SampleId) -> Result<usize, CommandError> {
        let found = self.project.samples.iter().position(|s| s.id == id);
        found.ok_or_else(|| CommandError::not_found("sample", id))
    }

    fn channel_index(&self, id: ChannelId) -> Result<usize, CommandError> {
        let found = self.project.channels.iter().position(|c| c.id == id);
        found.ok_or_else(|| CommandError::not_found("channel", id))
    }

    fn pattern_index(&self, id: PatternId) -> Result<usize, CommandError> {
        let found = self.project.patterns.iter().position(|p| p.id == id);
        found.ok_or_else(|| CommandError::not_found("pattern", id))
    }

    fn mixer_track_index(&self, id: TrackId) -> Result<usize, CommandError> {
        let found = self.project.mixer.tracks.iter().position(|t| t.id == id);
        found.ok_or_else(|| CommandError::not_found("mixer track", id))
    }

    fn signal_track_index(&self, id: TrackId) -> Result<usize, CommandError> {
        let index = self.mixer_track_index(id)?;
        if self.project.mixer.tracks[index].current {
            return Err(CommandError::invalid(
                "Current follows selection and cannot be an audio routing destination",
            ));
        }
        Ok(index)
    }

    fn playlist_track_index(&self, id: PlaylistTrackId) -> Result<usize, CommandError> {
        let found = self.project.playlist.tracks.iter().position(|t| t.id == id);
        found.ok_or_else(|| CommandError::not_found("playlist track", id))
    }

    /// The channel's lane in the pattern, or `None` when it has no notes
    /// there. Fails when the pattern or the channel does not exist.
    fn lane(&self, pattern: PatternId, channel: ChannelId) -> Result<Option<&Lane>, CommandError> {
        let pattern = &self.project.patterns[self.pattern_index(pattern)?];
        self.channel_index(channel)?;
        Ok(pattern.lane(channel))
    }

    /// The notes a channel has in each pattern where it has any.
    fn lanes_of(&self, channel: ChannelId) -> Vec<(PatternId, Vec<Note>)> {
        let patterns = self.project.patterns.iter();
        patterns
            .filter_map(|pattern| Some((pattern.id, pattern.lane(channel)?.notes.clone())))
            .collect()
    }

    fn clips_where(&self, keep: impl Fn(&Clip) -> bool) -> Vec<Clip> {
        let clips = self.project.playlist.clips.iter();
        clips.filter(|clip| keep(clip)).cloned().collect()
    }

    /// The index of a mixer track and, in its chain, of one of its effects.
    /// An effect that sits on another track is not found.
    fn effect_index(
        &self,
        track: TrackId,
        effect: EffectId,
    ) -> Result<(usize, usize), CommandError> {
        let track = self.mixer_track_index(track)?;
        let effects = &self.project.mixer.tracks[track].effects;
        let found = effects.iter().position(|slot| slot.id == effect);
        found
            .map(|slot| (track, slot))
            .ok_or_else(|| CommandError::not_found("effect", effect))
    }

    /// How many effects the mixer track at `index` holds, when it can take
    /// one more.
    fn room_for_effect(&self, index: usize) -> Result<usize, CommandError> {
        let track = &self.project.mixer.tracks[index];
        if track.effects.len() >= MAX_EFFECT_SLOTS {
            return Err(CommandError::invalid(format!(
                "the mixer track \"{}\" is full: it holds {MAX_EFFECT_SLOTS} effects at most",
                track.name
            )));
        }
        Ok(track.effects.len())
    }

    /// The index of the mixer track that is `channel`'s own and still has
    /// the channel's name, as the track made with a channel does: a rename
    /// of the channel is then a rename of both. A track is the channel's
    /// own when nothing else plays into it: no other channel, no audio
    /// clip, and no other track through its output or a send. The master
    /// is nobody's own, and a track the user gave another name keeps it.
    fn track_named_after(&self, channel: &Channel) -> Option<usize> {
        let project = &*self.project;
        let id = channel.mixer_track;
        let tracks = &project.mixer.tracks;
        let index = tracks.iter().position(|track| track.id == id)?;
        let shared = id == TrackId::MASTER
            || tracks[index].name != channel.name
            || project
                .channels
                .iter()
                .any(|other| other.id != channel.id && other.mixer_track == id)
            || project.playlist.clips.iter().any(|clip| {
                matches!(clip.content, ClipContent::Audio { mixer_track, .. } if mixer_track == id)
            })
            || tracks.iter().any(|track| {
                track.output == Some(id) || track.sends.iter().any(|send| send.target == id)
            });
        (!shared).then_some(index)
    }

    /// Edits the mixer track at `index`.
    fn change_track(&mut self, index: usize, change: impl FnOnce(&mut MixerTrack)) {
        let old = self.project.mixer.tracks[index].clone();
        let mut new = old.clone();
        change(&mut new);
        self.push(Edit::SetMixerTrack(Box::new(Change { old, new })));
    }

    /// The sampler settings of the channel at `index`. Fails on a channel
    /// that plays an instrument.
    fn sampler(&self, index: usize) -> Result<SamplerSettings, CommandError> {
        let channel = &self.project.channels[index];
        match &channel.source {
            ChannelSource::Sampler(sampler) => Ok(sampler.clone()),
            ChannelSource::Instrument { .. } => Err(CommandError::invalid(format!(
                "the channel \"{}\" plays an instrument, so it has no sampler settings",
                channel.name
            ))),
        }
    }

    fn set_sampler(&mut self, index: usize, sampler: SamplerSettings) {
        self.set_source(index, ChannelSource::Sampler(sampler));
    }

    /// The instrument settings of the channel at `index`. Fails on a
    /// sampler channel.
    fn instrument(&self, index: usize) -> Result<InstrumentParams, CommandError> {
        let channel = &self.project.channels[index];
        match &channel.source {
            ChannelSource::Instrument { params } => Ok(*params),
            ChannelSource::Sampler(_) => Err(CommandError::invalid(format!(
                "the channel \"{}\" is a sampler, so it has no instrument settings",
                channel.name
            ))),
        }
    }

    fn set_instrument(&mut self, index: usize, params: InstrumentParams) {
        self.set_source(index, ChannelSource::Instrument { params });
    }

    fn set_source(&mut self, index: usize, source: ChannelSource) {
        let old = self.project.channels[index].clone();
        let new = Channel {
            source,
            ..old.clone()
        };
        self.push(Edit::SetChannel(Box::new(Change { old, new })));
    }
}

/// The notes of a lane that `keep` accepts.
fn notes_where(lane: Option<&Lane>, keep: impl Fn(&Note) -> bool) -> Vec<Note> {
    let notes = lane.map_or(&[][..], |lane| lane.notes.as_slice());
    notes.iter().filter(|note| keep(note)).copied().collect()
}

fn new_mixer_track(id: TrackId, name: String, color: u32) -> MixerTrack {
    MixerTrack {
        dock: crate::MixerDock::default(),
        external_output: None,
        processing: windfall_dsp::TrackParams::default(),
        current: false,
        latency_offset_ms: 0.0,
        id,
        name,
        color,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        output: Some(TrackId::MASTER),
        sidechains: Vec::new(),
        sends: Vec::new(),
        effects: Vec::new(),
        recording: None,
    }
}

/// True for a target that is a setting, or the mix, of this effect.
fn targets_effect(target: &AutomationTarget, id: EffectId) -> bool {
    match *target {
        AutomationTarget::EffectParam { effect, .. }
        | AutomationTarget::EffectMix { effect, .. } => effect == id,
        _ => false,
    }
}

/// The points of a curve, with every value and curve inside its range.
fn checked_points(points: Vec<AutomationPoint>) -> Result<Vec<AutomationPoint>, CommandError> {
    if points.is_empty() {
        return Err(CommandError::invalid(
            "an automation needs at least one point",
        ));
    }
    if points.len() > MAX_AUTOMATION_POINTS {
        return Err(CommandError::invalid(format!(
            "an automation can have {MAX_AUTOMATION_POINTS} points at most, not {}",
            points.len()
        )));
    }
    if !points.is_sorted_by_key(|point| point.tick) {
        return Err(CommandError::invalid(
            "the points of an automation must be in order of time",
        ));
    }
    let longest = MAX_SONG_TICKS / PPQ;
    if points
        .last()
        .is_some_and(|point| point.tick > MAX_SONG_TICKS)
    {
        return Err(CommandError::invalid(format!(
            "a point of the automation is past the end of the longest song, which has {longest} beats"
        )));
    }
    points
        .into_iter()
        .map(|point| {
            Ok(AutomationPoint {
                value: clamped("the value of a point", point.value, 0.0, 1.0)?,
                curve: clamped("the curve of a point", point.curve, -1.0, 1.0)?,
                ..point
            })
        })
        .collect()
}

/// A value for one setting of an effect or instrument, ready for the
/// setting to bring into its own range. Infinity means the end of the range.
/// NaN has no nearest valid value, so it is an error.
fn checked_setting(value: f32) -> Result<f32, CommandError> {
    if value.is_nan() {
        return Err(CommandError::invalid("the value is not a number"));
    }
    // Adding zero turns -0.0 into 0.0, which keeps "-0.0" out of project files.
    Ok(value.clamp(f32::MIN, f32::MAX) + 0.0)
}

/// Brings every setting of an effect into its range, the way setting them
/// one by one would.
fn checked_effect(mut params: EffectParams) -> Result<EffectParams, CommandError> {
    for index in 0..params.kind().descriptors().len() {
        if let Some(value) = params.get(index) {
            params.set(index, checked_setting(value)?);
        }
    }
    let params = params.sanitized();
    // What no setting reaches is not cleaned by the loop above.
    if params.sanitized() != params {
        return Err(CommandError::invalid(
            "the settings hold a value that is not a number",
        ));
    }
    Ok(params)
}

/// Brings every setting of an instrument into its range, the way setting
/// them one by one would.
fn checked_instrument(mut params: InstrumentParams) -> Result<InstrumentParams, CommandError> {
    for index in 0..params.kind().descriptors().len() {
        if let Some(value) = params.get(index) {
            params.set(index, checked_setting(value)?);
        }
    }
    let params = params.sanitized();
    if params.sanitized() != params {
        return Err(CommandError::invalid(
            "the settings hold a value that is not a number",
        ));
    }
    Ok(params)
}

/// The history label for a change to the setting at `param` of a processor
/// with these descriptors: "Change" and the setting's name, as in "Change
/// Cutoff" or "Change Osc 1 level".
fn setting_label(descriptors: &[ParamInfo], param: u32) -> String {
    match descriptors.get(param as usize) {
        Some(info) => format!("Change {}", info.name),
        None => "Change setting".to_owned(),
    }
}

fn no_setting(processor: &str, param: u32) -> CommandError {
    CommandError::invalid(format!("the {processor} has no setting number {param}"))
}

/// The error for settings of one kind of processor given to another kind.
fn other_kind(given: &str, held: &str) -> CommandError {
    CommandError::invalid(format!(
        "these are the settings of a {given}, and this is a {held}"
    ))
}

/// Brings a value into its range. NaN has no nearest valid value, so it is
/// an error.
fn clamped(what: &str, value: f32, min: f32, max: f32) -> Result<f32, CommandError> {
    if value.is_nan() {
        return Err(CommandError::invalid(format!("{what} is not a number")));
    }
    // Adding zero turns -0.0 into 0.0, which keeps "-0.0" out of project files.
    Ok(value.clamp(min, max) + 0.0)
}

fn checked_group_name(name: &str) -> Result<String, CommandError> {
    let name = name.trim();
    if name.len() > crate::MAX_CHANNEL_GROUP_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(CommandError::invalid(
            "a channel group name needs at most 128 UTF-8 bytes and no control characters",
        ));
    }
    Ok(name.to_owned())
}

fn checked_color(color: u32) -> Result<u32, CommandError> {
    if color > 0xFF_FFFF {
        return Err(CommandError::invalid(format!(
            "{color:#x} is not a 0xRRGGBB color"
        )));
    }
    Ok(color)
}

fn checked_key(key: u8) -> Result<u8, CommandError> {
    if key > MAX_KEY {
        return Err(CommandError::invalid(format!(
            "key {key} is above the highest key, {MAX_KEY}"
        )));
    }
    Ok(key)
}

fn checked_note(note: Note) -> Result<Note, CommandError> {
    if ![
        note.expression.release,
        note.expression.fine_pitch_cents,
        note.expression.modulation_x,
        note.expression.modulation_y,
    ]
    .iter()
    .all(|value| value.is_finite())
    {
        return Err(CommandError::invalid("note expression must be finite"));
    }
    if note.length == 0 {
        return Err(CommandError::invalid(
            "a note must be at least one tick long",
        ));
    }
    if note.start.checked_add(note.length).is_none() {
        return Err(CommandError::invalid("the note ends past the last tick"));
    }
    Ok(Note {
        key: checked_key(note.key)?,
        velocity: clamped("the velocity", note.velocity, 0.0, 1.0)?,
        pan: clamped("the pan", note.pan, -1.0, 1.0)?,
        expression: note.expression.clamped(),
        ..note
    })
}

/// A note that an edit puts at a new start: [`checked_note`], and the start
/// has to lie inside the longest pattern.
fn placed_note(note: Note) -> Result<Note, CommandError> {
    if note.start >= MAX_PATTERN_TICKS {
        return Err(CommandError::invalid(format!(
            "the note would start at step {}, past the end of the longest pattern, which has {MAX_PATTERN_STEPS} steps",
            note.start / TICKS_PER_STEP + 1
        )));
    }
    checked_note(note)
}

fn checked_clip(clip: Clip) -> Result<Clip, CommandError> {
    if clip.length == 0 {
        return Err(CommandError::invalid(
            "a clip must be at least one tick long",
        ));
    }
    let longest = MAX_SONG_TICKS / PPQ;
    if u64::from(clip.start) + u64::from(clip.length) > u64::from(MAX_SONG_TICKS) {
        return Err(CommandError::invalid(format!(
            "the clip would end past the end of the longest song, which has {longest} beats"
        )));
    }
    if clip.offset > MAX_SONG_TICKS {
        return Err(CommandError::invalid(format!(
            "the clip would start further into what it plays than the longest song lasts, which is {longest} beats"
        )));
    }
    Ok(clip)
}

/// The error for the first id of `wanted` that is not among `found`.
fn first_missing<I: Copy + Eq + Hash + Into<u32>>(
    kind: &'static str,
    wanted: impl IntoIterator<Item = I>,
    found: impl IntoIterator<Item = I>,
) -> CommandError {
    let found: HashSet<I> = found.into_iter().collect();
    let missing = wanted.into_iter().find(|id| !found.contains(id));
    CommandError::not_found(kind, missing.map_or(0, Into::into))
}

/// The label of the one field a patch sets, or `many` when it sets several
/// or none.
fn single_label(fields: &[(bool, Label)], many: Label) -> Label {
    let mut set = fields.iter().filter(|(is_set, _)| *is_set);
    match (set.next(), set.next()) {
        (Some((_, label)), None) => label,
        _ => many,
    }
}

fn plural(count: usize, one: Label, many: Label) -> Label {
    if count == 1 { one } else { many }
}

/// "`prefix` N" for the first N from `start` up that no existing name uses.
fn numbered_name<'a>(
    prefix: &str,
    start: usize,
    existing: impl Iterator<Item = &'a str>,
) -> String {
    let taken: HashSet<&str> = existing.collect();
    (start..)
        .map(|number| format!("{prefix} {number}"))
        .find(|name| !taken.contains(name.as_str()))
        .unwrap_or_else(|| prefix.to_owned())
}

/// `name` if no existing name is the same, and otherwise "`name` N" for
/// the first N from 2 up that is free. Capitals make no difference to what
/// counts as the same.
fn unused_name<'a>(name: String, existing: impl Iterator<Item = &'a str>) -> String {
    let taken: Vec<&str> = existing.collect();
    let free = |name: &str| !taken.iter().any(|held| held.eq_ignore_ascii_case(name));
    if free(&name) {
        return name;
    }
    let numbered = (2..).map(|number| format!("{name} {number}"));
    numbered
        .into_iter()
        .find(|numbered| free(numbered))
        .unwrap_or(name)
}

/// The name for a copy: "Kick" becomes "Kick #2", and "Kick #2" becomes
/// "Kick #3", skipping names already in use.
fn copy_name<'a>(original: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let base = match original.rsplit_once(" #") {
        Some((base, number)) if number.parse::<u32>().is_ok() => base,
        _ => original,
    };
    let taken: HashSet<&str> = existing.collect();
    (2..)
        .map(|number| format!("{base} #{number}"))
        .find(|name| !taken.contains(name.as_str()))
        .unwrap_or_else(|| original.to_owned())
}

#[cfg(test)]
mod settings_tests {
    use crate::{Command, CommandError, Document, Project, ProjectSettings, SettingsPatch};

    fn apply(doc: &mut Document, patch: SettingsPatch) {
        doc.dispatch(Command::UpdateSettings { patch }, None)
            .unwrap();
    }

    #[test]
    fn settings_metadata_defaults_when_omitted_and_omits_empty_fields() {
        let project = Project::new("Old project");
        let json = serde_json::to_value(&project).unwrap();
        for field in ["author", "genre", "comments"] {
            assert!(json["settings"].get(field).is_none());
        }
        let loaded: Project = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.settings, project.settings);
        assert!(loaded.settings.author.is_empty());
        assert!(loaded.settings.genre.is_empty());
        assert!(loaded.settings.comments.is_empty());
        assert_eq!(
            serde_json::from_str::<SettingsPatch>("{}").unwrap(),
            SettingsPatch::default()
        );
    }

    #[test]
    fn settings_metadata_caps_count_trimmed_utf8_bytes_and_reject_atomically() {
        for (field, cap) in [("author", 256), ("genre", 128), ("comments", 16_384)] {
            let mut doc = Document::new(Project::new("Caps"));
            let text = "é".repeat(cap / 2);
            let patch = serde_json::from_value(serde_json::json!({ field: format!("  {text}\n") }))
                .unwrap();
            apply(&mut doc, patch);
            let saved = serde_json::to_value(&doc.project().settings).unwrap();
            assert_eq!(saved[field], text);
            let before = doc.project().clone();
            let history = doc.history();
            let patch = serde_json::from_value(serde_json::json!({
                field: format!("{text}x"), "name": "Must not change"
            }))
            .unwrap();
            let error = doc
                .dispatch(Command::UpdateSettings { patch }, None)
                .unwrap_err();
            assert_eq!(
                error,
                CommandError::Invalid(format!("the project {field} is longer than {cap} bytes"))
            );
            assert_eq!(doc.project(), &before);
            assert_eq!(doc.history(), history);
        }
    }

    #[test]
    fn settings_metadata_patch_is_one_undo_step_and_round_trips() {
        let mut doc = Document::new(Project::new("Metadata"));
        let before = doc.project().settings.clone();
        apply(
            &mut doc,
            SettingsPatch {
                author: Some("  Windfall artist  ".into()),
                genre: Some("  Ambient  ".into()),
                comments: Some("  First line\nSecond line  ".into()),
                ..SettingsPatch::default()
            },
        );
        let edited = doc.project().settings.clone();
        assert_eq!(edited.author, "Windfall artist");
        assert_eq!(edited.genre, "Ambient");
        assert_eq!(edited.comments, "First line\nSecond line");
        assert_eq!(doc.history().entries.len(), 1);
        let loaded: ProjectSettings =
            serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
        assert_eq!(loaded, edited);
        doc.undo().unwrap();
        assert_eq!(doc.project().settings, before);
        assert!(doc.undo().is_none());
        doc.redo().unwrap();
        assert_eq!(doc.project().settings, edited);
    }

    #[test]
    fn settings_comments_edit_undo_and_clear() {
        let mut doc = Document::new(Project::new("Comments"));
        apply(
            &mut doc,
            SettingsPatch {
                comments: Some("Notes".into()),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(doc.history().entries[0].label, "Change project comments");
        doc.undo().unwrap();
        assert!(doc.project().settings.comments.is_empty());
        doc.redo().unwrap();
        apply(
            &mut doc,
            SettingsPatch {
                comments: Some(" \n\t ".into()),
                ..SettingsPatch::default()
            },
        );
        assert!(doc.project().settings.comments.is_empty());
        doc.undo().unwrap();
        assert_eq!(doc.project().settings.comments, "Notes");
    }

    #[test]
    fn settings_empty_patch_preserves_history_dirty_state_and_redo() {
        let mut doc = Document::new(Project::new("Empty"));
        apply(
            &mut doc,
            SettingsPatch {
                comments: Some("Notes".into()),
                ..SettingsPatch::default()
            },
        );
        doc.undo().unwrap();
        let before = doc.project().clone();
        let history = doc.history();
        let applied = doc
            .dispatch(
                Command::UpdateSettings {
                    patch: SettingsPatch::default(),
                },
                None,
            )
            .unwrap();
        assert!(applied.touched.is_empty());
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
        assert!(!doc.is_dirty());
        doc.redo().unwrap();
        assert_eq!(doc.project().settings.comments, "Notes");
    }
}
