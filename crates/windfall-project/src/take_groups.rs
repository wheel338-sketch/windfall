//! Saved recording associations; source audio remains ordinary playlist clips.
use crate::{ClipContent, ClipId, Project};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

pub const MAX_TAKE_GROUPS: usize = 256;
pub const MAX_TAKE_GROUP_LANES: usize = crate::MAX_MIXER_TRACKS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioTakeGroup {
    /// A global project ID, allocated by the native command.
    pub id: u32,
    pub name: String,
    pub lanes: Vec<AudioTakeLane>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ClipId>>", optional)]
    pub comp: Vec<ClipId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioTakeLane {
    pub name: String,
    pub takes: Vec<AudioTakeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioTakeRef {
    pub pass: u16,
    pub clip: ClipId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TakeCompRange {
    pub pass: u16,
    pub start: u32,
    pub end: u32,
}

pub(crate) fn check_group(project: &Project, group: &AudioTakeGroup) -> Result<(), String> {
    if group.name.len() > 128
        || group.name.contains('\0')
        || group.lanes.len() > MAX_TAKE_GROUP_LANES
        || group.lanes.is_empty() && group.comp.is_empty()
        || group.comp.len() > 2048 * MAX_TAKE_GROUP_LANES
    {
        return Err("Take group name, lane count or composite exceeds its bounds".into());
    }
    let mut clips = HashSet::new();
    for lane in &group.lanes {
        if lane.name.len() > 128
            || lane.name.contains('\0')
            || lane.takes.is_empty()
            || lane.takes.len() > 256
        {
            return Err("Take lanes need a bounded name and 1–256 retained passes".into());
        }
        let mut passes = HashSet::new();
        for take in &lane.takes {
            if take.pass >= 256 || !passes.insert(take.pass) || !clips.insert(take.clip) {
                return Err("Each take lane needs unique pass numbers and source clips".into());
            }
        }
    }
    for clip in &group.comp {
        if !clips.insert(*clip) {
            return Err("A composite cannot repeat or reuse a take source ID".into());
        }
    }
    if clips.iter().any(|id| {
        !project
            .playlist
            .clips
            .iter()
            .any(|clip| clip.id == *id && matches!(clip.content, ClipContent::Audio { .. }))
    }) {
        return Err("Take group audio clips must exist in the playlist".into());
    }
    Ok(())
}

pub(crate) fn prune(project: &Project) -> Vec<AudioTakeGroup> {
    let live: HashSet<_> = project
        .playlist
        .clips
        .iter()
        .filter(|clip| matches!(clip.content, ClipContent::Audio { .. }))
        .map(|clip| clip.id)
        .collect();
    let mut groups = project.playlist.take_groups.clone();
    for group in &mut groups {
        for lane in &mut group.lanes {
            lane.takes.retain(|take| live.contains(&take.clip));
        }
        group.lanes.retain(|lane| !lane.takes.is_empty());
        group.comp.retain(|clip| live.contains(clip));
    }
    groups.retain(|group| !group.lanes.is_empty() || !group.comp.is_empty());
    groups
}
