//! Playlist layout edits that belong together: independent arrangement
//! layouts, an automation clip's own curve copy, and the linked tracks that
//! make a lane behave like an instrument or audio track.

use super::*;
use crate::arrangement::{Arrangement, ClipGroup, TrackKind};

/// The name a project's present timeline takes when it becomes the first of
/// several layouts.
const FIRST_LAYOUT_NAME: &str = "Arrangement 1";

impl Transaction<'_> {
    /// Adds a layout and switches to it. A copy owns new clip entities, so
    /// the two layouts no longer share positions, lengths or mute states.
    pub(super) fn add_arrangement_layout(
        &mut self,
        name: String,
        copy: bool,
    ) -> Result<Label, CommandError> {
        let invalid =
            |error: crate::arrangement::ArrangementError| CommandError::invalid(error.to_string());
        let mut book = self.project.playlist.arrangement_book.clone();
        book.check().map_err(invalid)?;
        if book.arrangements.is_empty() {
            // The timeline as it stands is the first layout, so nothing the
            // user has placed is hidden by adding a second one.
            let id = self.allocate()?;
            let playlist = &self.project.playlist;
            book.arrangements.push(Arrangement {
                id,
                name: FIRST_LAYOUT_NAME.to_owned(),
                clips: playlist.clips.iter().map(|clip| clip.id).collect(),
                tracks: playlist.tracks.iter().map(|track| track.id).collect(),
            });
            book.active = Some(id);
        }
        let source = book
            .arrangements
            .iter()
            .find(|item| Some(item.id) == book.active)
            .cloned()
            .ok_or_else(|| invalid(crate::arrangement::ArrangementError::InvalidActive))?;

        let id = self.allocate()?;
        let mut insert = Vec::new();
        let timeline = if !book.timelines.is_empty() {
            book.timelines
                .insert(source.id, self.project.playlist.timeline.clone());
            let mut timeline = if copy {
                self.project.playlist.timeline.clone()
            } else {
                crate::Timeline::default()
            };
            for meter in &mut timeline.meters {
                meter.id = crate::MeterChangeId(self.allocate()?);
            }
            for marker in &mut timeline.markers {
                marker.id = crate::TimelineMarkerId(self.allocate()?);
            }
            book.timelines.insert(id, timeline.clone());
            Some(timeline)
        } else {
            None
        };
        if copy {
            let mut copies = HashMap::new();
            for original in &source.clips {
                let Some(clip) = self
                    .project
                    .playlist
                    .clips
                    .iter()
                    .find(|clip| clip.id == *original)
                else {
                    continue;
                };
                let mut clip = clip.clone();
                clip.id = ClipId(self.allocate()?);
                copies.insert(*original, clip.id);
                insert.push(clip);
            }
            // A group whose members were copied is a group in the copy too.
            for group in book.clip_groups.clone() {
                let clips: Vec<ClipId> = group
                    .clips
                    .iter()
                    .filter_map(|clip| copies.get(clip).copied())
                    .collect();
                if clips.len() >= 2 {
                    book.clip_groups.push(ClipGroup {
                        id: self.allocate()?,
                        clips,
                    });
                }
            }
        }
        let clips: Vec<ClipId> = insert.iter().map(|clip| clip.id).collect();
        book.arrangements.push(Arrangement {
            id,
            name,
            clips: clips.clone(),
            tracks: source.tracks,
        });
        book.active = Some(id);
        book.check().map_err(invalid)?;

        let mut proposed = self.project.clone();
        proposed.playlist.clips.extend(insert.iter().cloned());
        proposed.playlist.clips.sort_by_key(Clip::sort_key);
        proposed.playlist.arrangement_book = book.clone();
        if let Some(timeline) = &timeline {
            proposed.playlist.timeline = timeline.clone();
        }
        proposed.check().map_err(CommandError::invalid)?;

        self.push(Edit::Clips {
            remove: Vec::new(),
            insert,
        });
        let old = self.project.playlist.arrangement_book.clone();
        self.push(Edit::ArrangementBook(Change { old, new: book }));
        if let Some(new) = timeline {
            let old = self.project.playlist.timeline.clone();
            self.push(Edit::Timeline(Change { old, new }));
        }
        self.created.push(id);
        self.created.extend(clips.iter().map(|clip| clip.0));
        Ok("Add arrangement")
    }

    /// Removes a layout together with the clips only it placed. Clips that
    /// another layout still lists stay, as do clips no layout ever listed.
    pub(super) fn remove_arrangement_layout(
        &mut self,
        id: crate::arrangement::ArrangementId,
    ) -> Result<Label, CommandError> {
        let book = &self.project.playlist.arrangement_book;
        let removed: Vec<ClipId> = book
            .arrangements
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.clips.clone())
            .unwrap_or_default();
        let next = book.remove_arrangement(id);
        let kept: HashSet<ClipId> = next
            .as_ref()
            .map(|book| {
                book.arrangements
                    .iter()
                    .flat_map(|item| item.clips.iter().copied())
                    .collect()
            })
            .unwrap_or_default();
        self.replace_arrangement_book(next, self.project.next_id)?;
        let orphaned: HashSet<ClipId> = removed
            .into_iter()
            .filter(|clip| !kept.contains(clip))
            .collect();
        if !orphaned.is_empty() {
            let remove = self.clips_where(|clip| orphaned.contains(&clip.id));
            self.push(Edit::Clips {
                remove,
                insert: Vec::new(),
            });
        }
        Ok("Remove arrangement")
    }

    /// Gives one automation clip a curve of its own: a copy of the
    /// automation with the same target and points, shown by this clip only.
    pub(super) fn make_automation_clip_unique(
        &mut self,
        old: Clip,
        automation: AutomationId,
    ) -> Result<(), CommandError> {
        let index = self.automation_index(automation)?;
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
        let created = copy.id;
        let mut new = old.clone();
        new.content = ClipContent::Automation {
            automation: created,
        };
        self.push(Edit::Automation(ListEdit::Insert {
            index: index + 1,
            item: copy,
        }));
        self.push(Edit::Clips {
            remove: vec![old],
            insert: vec![new],
        });
        self.created.push(created.0);
        Ok(())
    }

    pub(super) fn add_instrument_track(
        &mut self,
        name: Option<String>,
        sample: Option<SampleId>,
        instrument: Option<InstrumentKind>,
    ) -> Result<Label, CommandError> {
        let first = self.created.len();
        self.add_channel(name, sample, instrument, None, None)?;
        let channel = ChannelId(self.created[first]);
        let name = self.project.channels[self.channel_index(channel)?]
            .name
            .clone();
        let track = self.linked_playlist_track(name, TrackKind::Instrument { channel })?;
        self.created.push(track.0);
        Ok("Add instrument track")
    }

    pub(super) fn add_audio_track(&mut self, name: Option<String>) -> Result<Label, CommandError> {
        let first = self.created.len();
        self.add_mixer_track(name)?;
        let mixer = TrackId(self.created[first]);
        let name = self.project.mixer.tracks[self.mixer_track_index(mixer)?]
            .name
            .clone();
        let track = self.linked_playlist_track(name, TrackKind::AudioTrack { mixer })?;
        self.created.push(track.0);
        Ok("Add audio track")
    }

    /// A new lane at the end of the playlist, named after what it is linked
    /// to. The caller reports the id, after the ids of what it links.
    fn linked_playlist_track(
        &mut self,
        name: String,
        kind: TrackKind,
    ) -> Result<PlaylistTrackId, CommandError> {
        let reported = self.created.len();
        self.add_playlist_track(Some(name), None)?;
        let track = PlaylistTrackId(self.created[reported]);
        self.created.truncate(reported);
        let next = self
            .project
            .playlist
            .arrangement_book
            .link_track(track, Some(kind), |_| true);
        self.replace_arrangement_book(next, self.project.next_id)?;
        Ok(track)
    }

    /// Keeps linked tracks true after any command: a link to a mixer track
    /// that is gone is dropped, and every audio clip on an audio track plays
    /// into the mixer track that track is linked to. Runs inside the
    /// command's own transaction, so undo takes the routing back with it.
    pub(super) fn settle_linked_tracks(&mut self) {
        let book = &self.project.playlist.arrangement_book;
        if !book
            .linked_tracks
            .values()
            .any(|kind| matches!(kind, TrackKind::AudioTrack { .. }))
        {
            return;
        }
        let mixer = &self.project.mixer;
        let mut next = book.clone();
        next.linked_tracks.retain(|_, kind| match kind {
            TrackKind::AudioTrack { mixer: track } => {
                mixer.track(*track).is_some_and(|track| !track.current)
            }
            _ => true,
        });
        if next != *book {
            let old = book.clone();
            self.push(Edit::ArrangementBook(Change { old, new: next }));
        }

        let links = &self.project.playlist.arrangement_book.linked_tracks;
        let (remove, insert): (Vec<Clip>, Vec<Clip>) = self
            .project
            .playlist
            .clips
            .iter()
            .filter_map(|clip| {
                let Some(TrackKind::AudioTrack { mixer }) = links.get(&clip.track) else {
                    return None;
                };
                let mut routed = clip.clone();
                match &mut routed.content {
                    ClipContent::Audio { mixer_track, .. } if mixer_track != mixer => {
                        *mixer_track = *mixer;
                    }
                    _ => return None,
                }
                Some((clip.clone(), routed))
            })
            .unzip();
        if !remove.is_empty() {
            self.push(Edit::Clips { remove, insert });
        }
    }
}
