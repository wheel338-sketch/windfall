//! Pure arrangement metadata. The caller owns IDs, history, persistence and playback.
use crate::model::{
    ChannelId, Clip, ClipId, MAX_SONG_TICKS, Pattern, PatternId, PlaylistTrackId, SampleAsset,
    SampleId, TrackId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use ts_rs::TS;

pub type ArrangementId = u32;
pub type TrackGroupId = u32;
pub type ClipGroupId = u32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct ArrangementBook {
    pub arrangements: Vec<Arrangement>,
    /// None only for the legacy/default empty book.
    pub active: Option<ArrangementId>,
    pub track_groups: Vec<TrackGroup>,
    /// Child group -> parent group. Roots have no entry.
    pub group_parents: BTreeMap<TrackGroupId, TrackGroupId>,
    /// Playlist track -> owning group. Ungrouped tracks have no entry.
    pub track_parents: BTreeMap<PlaylistTrackId, TrackGroupId>,
    pub clip_groups: Vec<ClipGroup>,
    pub linked_tracks: BTreeMap<PlaylistTrackId, TrackKind>,
    /// Opt-in independent timelines. An empty map preserves legacy sharing.
    /// The active entry is a snapshot; playlist.timeline owns current edits.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<ArrangementId, crate::Timeline>>", optional)]
    pub timelines: BTreeMap<ArrangementId, crate::Timeline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub id: ArrangementId,
    pub name: String,
    #[serde(default)]
    pub clips: Vec<ClipId>,
    #[serde(default)]
    pub tracks: Vec<PlaylistTrackId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrackGroup {
    pub id: TrackGroupId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipGroup {
    pub id: ClipGroupId,
    pub clips: Vec<ClipId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TrackKind {
    /// An instrument track: pattern clips on it play only this channel,
    /// through the mixer track the channel is routed to.
    Instrument { channel: ChannelId },
    /// A label naming the audio source a lane is kept for. It routes nothing.
    Audio { source: SampleId },
    /// An audio track: audio clips on it play into this mixer track.
    AudioTrack { mixer: TrackId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SourceReference {
    Pattern { pattern: PatternId },
    Audio { source: SampleId },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArrangementError {
    #[error("{kind} ID {id} does not exist")]
    MissingId { kind: &'static str, id: u32 },
    #[error("{kind} ID {id} is duplicated or reused")]
    DuplicateId { kind: &'static str, id: u32 },
    #[error("a name needs non-whitespace text and no NUL, up to 256 UTF-8 bytes")]
    InvalidName,
    #[error("the last arrangement cannot be deleted")]
    LastArrangement,
    #[error("the active arrangement must exist, or be absent for an empty book")]
    InvalidActive,
    #[error("track group parenting would create a cycle")]
    GroupCycle,
    #[error("a clip group needs at least two different clips")]
    TooFewClips,
    #[error("clip {0:?} already belongs to another group")]
    ClipAlreadyGrouped(ClipId),
    #[error("clip {0:?} would move outside the song or available tracks")]
    MoveOutOfBounds(ClipId),
}

fn name_check(name: &str) -> Result<(), ArrangementError> {
    if name.trim().is_empty() || name.contains('\0') || name.len() > 256 {
        Err(ArrangementError::InvalidName)
    } else {
        Ok(())
    }
}

fn distinct<T: Copy + Ord + Into<u32>>(
    ids: impl IntoIterator<Item = T>,
    kind: &'static str,
) -> Result<(), ArrangementError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(ArrangementError::DuplicateId {
                kind,
                id: id.into(),
            });
        }
    }
    Ok(())
}

impl ArrangementBook {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Whether the layout that is active now shows this clip: it lists the
    /// clip and the lane the clip sits on. Without an active layout every
    /// clip is shown. Playback, export and consolidation follow this rule.
    pub fn shows(&self, clip: &Clip) -> bool {
        let Some(active) = self.active else {
            return true;
        };
        self.arrangements
            .iter()
            .find(|item| item.id == active)
            .is_none_or(|item| item.clips.contains(&clip.id) && item.tracks.contains(&clip.track))
    }

    /// Structural validation. The parent also validates references against project pools.
    pub fn check(&self) -> Result<(), ArrangementError> {
        distinct(self.arrangements.iter().map(|item| item.id), "arrangement")?;
        if self.arrangements.is_empty() != self.active.is_none()
            || self
                .active
                .is_some_and(|id| !self.arrangements.iter().any(|item| item.id == id))
        {
            return Err(ArrangementError::InvalidActive);
        }
        for item in &self.arrangements {
            name_check(&item.name)?;
            distinct(item.clips.iter().copied(), "clip")?;
            distinct(item.tracks.iter().copied(), "playlist track")?;
        }
        distinct(self.track_groups.iter().map(|item| item.id), "track group")?;
        for group in &self.track_groups {
            name_check(&group.name)?;
        }
        for (&child, &parent) in &self.group_parents {
            self.require_group(child)?;
            self.require_group(parent)?;
            let mut seen = BTreeSet::from([child]);
            let mut cursor = Some(parent);
            while let Some(id) = cursor {
                if !seen.insert(id) {
                    return Err(ArrangementError::GroupCycle);
                }
                cursor = self.group_parents.get(&id).copied();
            }
        }
        for parent in self.track_parents.values() {
            self.require_group(*parent)?;
        }
        distinct(self.clip_groups.iter().map(|item| item.id), "clip group")?;
        let mut grouped = BTreeSet::new();
        for group in &self.clip_groups {
            if group.clips.len() < 2 {
                return Err(ArrangementError::TooFewClips);
            }
            distinct(group.clips.iter().copied(), "clip")?;
            for &clip in &group.clips {
                if !grouped.insert(clip) {
                    return Err(ArrangementError::ClipAlreadyGrouped(clip));
                }
            }
        }
        Ok(())
    }

    fn require_group(&self, id: TrackGroupId) -> Result<(), ArrangementError> {
        if self.track_groups.iter().any(|group| group.id == id) {
            Ok(())
        } else {
            Err(ArrangementError::MissingId {
                kind: "track group",
                id,
            })
        }
    }

    /// Checks input and result. Errors never partially modify self.
    fn changed(
        &self,
        edit: impl FnOnce(&mut Self) -> Result<(), ArrangementError>,
    ) -> Result<Self, ArrangementError> {
        self.check()?;
        let mut next = self.clone();
        edit(&mut next)?;
        next.check()?;
        Ok(next)
    }

    pub fn add_arrangement(&self, arrangement: Arrangement) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            if next.active.is_none() {
                next.active = Some(arrangement.id);
            }
            next.arrangements.push(arrangement);
            Ok(())
        })
    }

    pub fn rename_arrangement(
        &self,
        id: ArrangementId,
        name: String,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.arrangement_mut(id)?.name = name;
            Ok(())
        })
    }

    pub fn set_arrangement_references(
        &self,
        id: ArrangementId,
        clips: Vec<ClipId>,
        tracks: Vec<PlaylistTrackId>,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            let item = next.arrangement_mut(id)?;
            item.clips = clips;
            item.tracks = tracks;
            Ok(())
        })
    }

    fn arrangement_mut(&mut self, id: ArrangementId) -> Result<&mut Arrangement, ArrangementError> {
        self.arrangements
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(ArrangementError::MissingId {
                kind: "arrangement",
                id,
            })
    }

    pub fn switch_arrangement(&self, id: ArrangementId) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.arrangement_mut(id)?;
            next.active = Some(id);
            Ok(())
        })
    }

    /// Deleting the active arrangement selects its next neighbour, or previous last.
    pub fn remove_arrangement(&self, id: ArrangementId) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            let index = next
                .arrangements
                .iter()
                .position(|item| item.id == id)
                .ok_or(ArrangementError::MissingId {
                    kind: "arrangement",
                    id,
                })?;
            if next.arrangements.len() == 1 {
                return Err(ArrangementError::LastArrangement);
            }
            next.arrangements.remove(index);
            next.timelines.remove(&id);
            if next.active == Some(id) {
                next.active = Some(next.arrangements[index.min(next.arrangements.len() - 1)].id);
            }
            Ok(())
        })
    }

    pub fn add_track_group(
        &self,
        group: TrackGroup,
        parent: Option<TrackGroupId>,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            if let Some(parent) = parent {
                next.require_group(parent)?;
                next.group_parents.insert(group.id, parent);
            }
            next.track_groups.push(group);
            Ok(())
        })
    }

    pub fn rename_track_group(
        &self,
        id: TrackGroupId,
        name: String,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.require_group(id)?;
            next.track_groups
                .iter_mut()
                .find(|group| group.id == id)
                .unwrap()
                .name = name;
            Ok(())
        })
    }

    pub fn move_track_group(
        &self,
        id: TrackGroupId,
        parent: Option<TrackGroupId>,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.require_group(id)?;
            if let Some(parent) = parent {
                next.require_group(parent)?;
                next.group_parents.insert(id, parent);
            } else {
                next.group_parents.remove(&id);
            }
            Ok(())
        })
    }

    pub fn move_track_to_group(
        &self,
        track: PlaylistTrackId,
        parent: Option<TrackGroupId>,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            if let Some(parent) = parent {
                next.require_group(parent)?;
                next.track_parents.insert(track, parent);
            } else {
                next.track_parents.remove(&track);
            }
            Ok(())
        })
    }

    /// Dissolves the group, promoting children and member tracks to its parent.
    /// Track identities, arrangement order and links are retained.
    pub fn remove_track_group(&self, id: TrackGroupId) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.require_group(id)?;
            let parent = next.group_parents.remove(&id);
            next.track_groups.retain(|group| group.id != id);
            next.group_parents.retain(|_, value| {
                if *value != id {
                    return true;
                }
                if let Some(parent) = parent {
                    *value = parent;
                    true
                } else {
                    false
                }
            });
            next.track_parents.retain(|_, value| {
                if *value != id {
                    return true;
                }
                if let Some(parent) = parent {
                    *value = parent;
                    true
                } else {
                    false
                }
            });
            Ok(())
        })
    }

    pub fn add_clip_group(&self, group: ClipGroup) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            next.clip_groups.push(group);
            Ok(())
        })
    }

    pub fn remove_clip_group(&self, id: ClipGroupId) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            if !next.clip_groups.iter().any(|group| group.id == id) {
                return Err(ArrangementError::MissingId {
                    kind: "clip group",
                    id,
                });
            }
            next.clip_groups.retain(|group| group.id != id);
            Ok(())
        })
    }

    /// Expansion for selection, deletion and movement; original selection first,
    /// then remaining members in group order, without duplicates.
    pub fn expand_clip_selection(
        &self,
        selected: &[ClipId],
    ) -> Result<Vec<ClipId>, ArrangementError> {
        self.check()?;
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for &clip in selected {
            if seen.insert(clip) {
                result.push(clip);
            }
        }
        for group in &self.clip_groups {
            if group.clips.iter().any(|clip| selected.contains(clip)) {
                for &clip in &group.clips {
                    if seen.insert(clip) {
                        result.push(clip);
                    }
                }
            }
        }
        Ok(result)
    }

    /// Returns the entire updated clip pool, sorted like Playlist.clips. Moves
    /// every grouped member by the same delta or refuses the entire move.
    pub fn move_clips(
        &self,
        clips: &[Clip],
        selected: &[ClipId],
        tick_delta: i64,
        track_delta: i32,
        tracks: &[PlaylistTrackId],
    ) -> Result<Vec<Clip>, ArrangementError> {
        distinct(clips.iter().map(|clip| clip.id), "clip")?;
        distinct(tracks.iter().copied(), "playlist track")?;
        let selected = self.expand_clip_selection(selected)?;
        let mut next = clips.to_vec();
        for id in selected {
            let clip =
                next.iter_mut()
                    .find(|clip| clip.id == id)
                    .ok_or(ArrangementError::MissingId {
                        kind: "clip",
                        id: id.0,
                    })?;
            let start = i64::from(clip.start)
                .checked_add(tick_delta)
                .filter(|&start| {
                    start >= 0 && start <= i64::from(MAX_SONG_TICKS) - i64::from(clip.length)
                })
                .ok_or(ArrangementError::MoveOutOfBounds(id))?;
            let index = tracks.iter().position(|&track| track == clip.track).ok_or(
                ArrangementError::MissingId {
                    kind: "playlist track",
                    id: clip.track.0,
                },
            )?;
            let index = i64::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(i64::from(track_delta)))
                .and_then(|index| usize::try_from(index).ok())
                .filter(|&index| index < tracks.len())
                .ok_or(ArrangementError::MoveOutOfBounds(id))?;
            if clip.length == 0 {
                return Err(ArrangementError::MoveOutOfBounds(id));
            }
            clip.start = start as u32;
            clip.track = tracks[index];
        }
        next.sort_by_key(Clip::sort_key);
        Ok(next)
    }

    /// None removes a link. The callback checks against the caller's channel/sample
    /// pool. The parent validates the playlist track ID separately.
    pub fn link_track(
        &self,
        track: PlaylistTrackId,
        kind: Option<TrackKind>,
        valid: impl Fn(TrackKind) -> bool,
    ) -> Result<Self, ArrangementError> {
        self.changed(|next| {
            if let Some(kind) = kind {
                if !valid(kind) {
                    return Err(match kind {
                        TrackKind::Instrument { channel } => ArrangementError::MissingId {
                            kind: "channel",
                            id: channel.0,
                        },
                        TrackKind::Audio { source } => ArrangementError::MissingId {
                            kind: "sample",
                            id: source.0,
                        },
                        TrackKind::AudioTrack { mixer } => ArrangementError::MissingId {
                            kind: "mixer track",
                            id: mixer.0,
                        },
                    });
                }
                next.linked_tracks.insert(track, kind);
            } else {
                next.linked_tracks.remove(&track);
            }
            Ok(())
        })
    }
}

pub enum UniqueSource<'a> {
    Pattern(&'a Pattern),
    Audio(&'a SampleAsset),
}
#[derive(Debug, Clone, PartialEq)]
pub enum UniqueCopy {
    Pattern(Pattern),
    Audio(SampleAsset),
}

/// Returns a fresh source reference and its owned copy; never edits a Project.
/// Patterns get fresh note/curve/timeline identities. Audio clones the asset
/// reference with the same immutable file path, not an independent physical file.
/// Use a staged document allocator and commit its cursor only on success.
pub fn make_unique(
    source: UniqueSource<'_>,
    mut allocate: impl FnMut() -> u32,
) -> Result<(SourceReference, UniqueCopy), ArrangementError> {
    let mut used = BTreeSet::new();
    match &source {
        UniqueSource::Pattern(pattern) => {
            used.insert(pattern.id.0);
            used.extend(
                pattern
                    .lanes
                    .iter()
                    .flat_map(|lane| &lane.notes)
                    .map(|note| note.id.0),
            );
            used.extend(pattern.timeline.meters.iter().map(|meter| meter.id.0));
            used.extend(pattern.timeline.markers.iter().map(|marker| marker.id.0));
            used.extend(pattern.events.iter().map(|lane| lane.id.0));
        }
        UniqueSource::Audio(sample) => {
            used.insert(sample.id.0);
        }
    }
    let mut fresh = || {
        let id = allocate();
        if id == 0 || !used.insert(id) {
            Err(ArrangementError::DuplicateId {
                kind: "source copy",
                id,
            })
        } else {
            Ok(id)
        }
    };
    match source {
        UniqueSource::Pattern(pattern) => {
            distinct(
                pattern
                    .lanes
                    .iter()
                    .flat_map(|lane| &lane.notes)
                    .map(|note| note.id),
                "note",
            )?;
            let mut copy = pattern.clone();
            copy.id = PatternId(fresh()?);
            let mut notes = BTreeMap::new();
            for lane in &mut copy.lanes {
                for note in &mut lane.notes {
                    let id = fresh()?.into();
                    notes.insert(note.id, id);
                    note.id = id;
                }
            }
            for curve in &mut copy.note_curves {
                curve.note = *notes.get(&curve.note).ok_or(ArrangementError::MissingId {
                    kind: "note",
                    id: curve.note.0,
                })?;
            }
            for meter in &mut copy.timeline.meters {
                meter.id = fresh()?.into();
            }
            for marker in &mut copy.timeline.markers {
                marker.id = fresh()?.into();
            }
            for lane in &mut copy.events {
                lane.id = crate::AutomationId(fresh()?);
            }
            Ok((
                SourceReference::Pattern { pattern: copy.id },
                UniqueCopy::Pattern(copy),
            ))
        }
        UniqueSource::Audio(sample) => {
            let mut copy = sample.clone();
            copy.id = SampleId(fresh()?);
            Ok((
                SourceReference::Audio { source: copy.id },
                UniqueCopy::Audio(copy),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClipContent, Project, SamplePath};
    fn arrangement(id: u32) -> Arrangement {
        Arrangement {
            id,
            name: format!("Arrangement {id}"),
            clips: vec![ClipId(10)],
            tracks: vec![PlaylistTrackId(20)],
        }
    }
    fn group(id: u32) -> TrackGroup {
        TrackGroup {
            id,
            name: format!("Group {id}"),
        }
    }
    fn clip(id: u32, start: u32) -> Clip {
        Clip {
            id: ClipId(id),
            track: PlaylistTrackId(20),
            start,
            length: 100,
            offset: 0,
            muted: false,
            content: ClipContent::Pattern {
                pattern: PatternId(30),
            },
        }
    }
    #[test]
    fn cycles_are_refused_without_changing_the_book() {
        let book = ArrangementBook::default()
            .add_track_group(group(1), None)
            .unwrap()
            .add_track_group(group(2), Some(1))
            .unwrap()
            .add_track_group(group(3), Some(2))
            .unwrap();
        let before = book.clone();
        assert_eq!(
            book.move_track_group(1, Some(3)),
            Err(ArrangementError::GroupCycle)
        );
        assert_eq!(
            book.move_track_group(2, Some(2)),
            Err(ArrangementError::GroupCycle)
        );
        assert_eq!(book, before);
    }
    #[test]
    fn last_arrangement_is_retained_and_switching_is_pure() {
        let book = ArrangementBook::default()
            .add_arrangement(arrangement(1))
            .unwrap();
        assert_eq!(
            book.remove_arrangement(1),
            Err(ArrangementError::LastArrangement)
        );
        let next = book
            .add_arrangement(arrangement(2))
            .unwrap()
            .switch_arrangement(2)
            .unwrap();
        assert_eq!(book.active, Some(1));
        assert_eq!(next.active, Some(2));
        assert_eq!(next.remove_arrangement(2).unwrap().active, Some(1));
        assert!(next.switch_arrangement(99).is_err());
        assert!(next.add_arrangement(arrangement(2)).is_err());
    }
    #[test]
    fn group_removal_preserves_tracks_children_and_links() {
        let book = ArrangementBook::default()
            .add_arrangement(arrangement(9))
            .unwrap()
            .add_track_group(group(1), None)
            .unwrap()
            .add_track_group(group(2), Some(1))
            .unwrap()
            .add_track_group(group(3), Some(2))
            .unwrap()
            .move_track_to_group(PlaylistTrackId(20), Some(2))
            .unwrap()
            .link_track(
                PlaylistTrackId(20),
                Some(TrackKind::Instrument {
                    channel: ChannelId(50),
                }),
                |_| true,
            )
            .unwrap();
        let next = book.remove_track_group(2).unwrap();
        assert_eq!(next.track_parents[&PlaylistTrackId(20)], 1);
        assert_eq!(next.group_parents[&3], 1);
        assert_eq!(next.arrangements, book.arrangements);
        assert_eq!(next.linked_tracks, book.linked_tracks);
        let next = next.remove_track_group(1).unwrap();
        assert!(next.track_parents.is_empty());
        assert!(next.group_parents.is_empty());
    }
    #[test]
    fn grouped_clips_move_as_a_set_and_refuse_atomically() {
        let book = ArrangementBook::default()
            .add_clip_group(ClipGroup {
                id: 1,
                clips: vec![ClipId(10), ClipId(11)],
            })
            .unwrap();
        let clips = vec![clip(10, 200), clip(11, 500), clip(12, 800)];
        let tracks = [PlaylistTrackId(20), PlaylistTrackId(21)];
        let moved = book
            .move_clips(&clips, &[ClipId(11)], 120, 1, &tracks)
            .unwrap();
        assert_eq!(moved[0].start, 320);
        assert_eq!(moved[1].start, 620);
        assert_eq!(moved[0].track, tracks[1]);
        assert_eq!(moved[1].track, tracks[1]);
        assert_eq!(moved[2], clips[2]);
        assert_eq!(clips[0].start, 200);
        assert!(
            book.move_clips(&clips, &[ClipId(11)], -201, 0, &tracks)
                .is_err()
        );
        assert!(
            book.move_clips(&clips, &[ClipId(10)], i64::MAX, 0, &tracks)
                .is_err()
        );
        assert!(
            book.move_clips(&clips, &[ClipId(10)], 0, 2, &tracks)
                .is_err()
        );
        assert!(
            book.move_clips(&clips[..1], &[ClipId(10)], 0, 0, &tracks)
                .is_err()
        );
        assert_eq!(
            book.expand_clip_selection(&[ClipId(11), ClipId(11)])
                .unwrap(),
            vec![ClipId(11), ClipId(10)]
        );
        assert!(
            book.add_clip_group(ClipGroup {
                id: 2,
                clips: vec![ClipId(11), ClipId(12)]
            })
            .is_err()
        );
    }
    #[test]
    fn unique_sources_have_new_identity_and_owned_data() {
        let project = Project::new("Unique source test");
        let pattern = &project.patterns[0];
        let mut next_id = project.next_id;
        let (reference, copy) = make_unique(UniqueSource::Pattern(pattern), || {
            let id = next_id;
            next_id += 1;
            id
        })
        .unwrap();
        let UniqueCopy::Pattern(mut copy) = copy else {
            panic!("expected pattern")
        };
        assert_ne!(copy.id, pattern.id);
        assert_eq!(reference, SourceReference::Pattern { pattern: copy.id });
        copy.name = "Independent".into();
        assert_ne!(copy.name, pattern.name);
        assert!(make_unique(UniqueSource::Pattern(pattern), || pattern.id.0).is_err());
        let sample = SampleAsset {
            id: SampleId(50),
            name: "Audio".into(),
            path: SamplePath::Project("audio.wav".into()),
        };
        let (reference, copy) = make_unique(UniqueSource::Audio(&sample), || 51).unwrap();
        assert_eq!(
            reference,
            SourceReference::Audio {
                source: SampleId(51)
            }
        );
        let UniqueCopy::Audio(copy) = copy else {
            panic!("expected audio")
        };
        assert_eq!(copy.path, sample.path);
        assert_ne!(copy.id, sample.id);
    }
    #[test]
    fn unique_pattern_remaps_notes_curves_and_timeline_ids() {
        let mut pattern = Project::new("Deep copy").patterns.remove(0);
        pattern.lanes.push(crate::model::Lane {
            channel: ChannelId(7),
            notes: vec![crate::model::Note {
                id: 10.into(),
                start: 0,
                length: 120,
                key: 60,
                velocity: 0.8,
                pan: 0.0,
                expression: Default::default(),
            }],
        });
        pattern.note_curves.push(
            serde_json::from_value(serde_json::json!({
                "note": 10, "parameter": "pan",
                "points": [{"position": 0.0, "value": 0.25, "curve": 0.0, "hold": false}]
            }))
            .unwrap(),
        );
        pattern.timeline.meters.push(
            serde_json::from_value(serde_json::json!({
                "id": 11, "tick": 0, "signature": {"numerator": 3, "denominator": 4}
            }))
            .unwrap(),
        );
        pattern.timeline.markers.push(
            serde_json::from_value(serde_json::json!({
                "id": 12, "tick": 0, "name": "Start", "kind": {"type": "named"}
            }))
            .unwrap(),
        );
        let before = pattern.clone();
        let mut cursor = 100;
        let (_, copied) = make_unique(UniqueSource::Pattern(&pattern), || {
            let id = cursor;
            cursor += 1;
            id
        })
        .unwrap();
        let UniqueCopy::Pattern(mut copy) = copied else {
            panic!("expected pattern")
        };
        assert_eq!(copy.id, PatternId(100));
        assert_eq!(copy.lanes[0].notes[0].id, 101.into());
        assert_eq!(copy.note_curves[0].note, copy.lanes[0].notes[0].id);
        assert_eq!(copy.timeline.meters[0].id.0, 102);
        assert_eq!(copy.timeline.markers[0].id.0, 103);
        assert_eq!(copy.lanes[0].channel, pattern.lanes[0].channel);
        copy.lanes[0].notes[0].key = 72;
        copy.note_curves[0].points[0].value = -0.5;
        assert_eq!(pattern, before);
        assert!(make_unique(UniqueSource::Pattern(&pattern), || 100).is_err());
    }

    #[test]
    fn missing_links_are_refused() {
        let book = ArrangementBook::default();
        let link = TrackKind::Audio {
            source: SampleId(99),
        };
        assert_eq!(
            book.link_track(PlaylistTrackId(20), Some(link), |_| false),
            Err(ArrangementError::MissingId {
                kind: "sample",
                id: 99
            })
        );
        let next = book
            .link_track(PlaylistTrackId(20), Some(link), |kind| kind == link)
            .unwrap();
        assert_eq!(next.linked_tracks[&PlaylistTrackId(20)], link);
        assert!(
            next.link_track(PlaylistTrackId(20), None, |_| false)
                .unwrap()
                .linked_tracks
                .is_empty()
        );
    }
    #[test]
    fn omitted_fields_load_empty_and_json_is_camel_case() {
        #[derive(Default, Deserialize)]
        #[serde(default, rename_all = "camelCase")]
        struct Container {
            arrangement_book: ArrangementBook,
        }
        let legacy: Container = serde_json::from_str("{}").unwrap();
        assert!(legacy.arrangement_book.is_empty());
        let book: ArrangementBook = serde_json::from_str("{}").unwrap();
        assert!(book.is_empty());
        let json = serde_json::to_value(book.add_track_group(group(1), None).unwrap()).unwrap();
        assert!(json.get("trackGroups").is_some());
        assert!(json.get("groupParents").is_some());
        assert!(json.get("track_groups").is_none());
        assert_eq!(
            serde_json::from_value::<ArrangementBook>(json)
                .unwrap()
                .track_groups,
            vec![group(1)]
        );
        let partial: ArrangementBook =
            serde_json::from_str(r#"{"arrangements":[{"id":1,"name":"A"}],"active":1}"#).unwrap();
        partial.check().unwrap();
        assert!(partial.arrangements[0].clips.is_empty());
    }
}
