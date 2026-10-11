//! Retained source clips become an editable composite, without rewriting audio.
use crate::{Clip, ClipContent, ClipId, CommandError, MAX_SONG_TICKS, Project};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioCompSegment {
    pub clip: ClipId,
    /// Absolute song ticks chosen from the source clip's visible range.
    pub start: u32,
    pub end: u32,
}

pub(crate) fn prepare(
    project: &Project,
    sources: &[Clip],
    segments: &[AudioCompSegment],
    fade_ticks: u32,
) -> Result<Vec<Clip>, CommandError> {
    if sources.is_empty() || sources.len() > 256 || segments.is_empty() || segments.len() > 2048 {
        return Err(CommandError::invalid(
            "Choose 1–256 source clips and 1–2048 comp ranges",
        ));
    }
    let mut captured = HashMap::new();
    for source in sources {
        let current = project
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id == source.id);
        if current != Some(source) || !matches!(source.content, ClipContent::Audio { .. }) {
            return Err(CommandError::invalid(
                "A comp source changed or disappeared; reopen the comp editor",
            ));
        }
        if captured.insert(source.id, source).is_some() {
            return Err(CommandError::invalid("Comp sources must be unique"));
        }
    }
    let mut sorted = segments.to_vec();
    sorted.sort_by_key(|segment| (segment.start, segment.end));
    let mut segments: Vec<AudioCompSegment> = Vec::with_capacity(sorted.len());
    for segment in sorted {
        if let Some(previous) = segments.last_mut()
            && previous.clip == segment.clip
            && previous.end == segment.start
        {
            previous.end = segment.end;
        } else {
            segments.push(segment);
        }
    }
    let mut prepared = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let source = captured
            .get(&segment.clip)
            .ok_or_else(|| CommandError::invalid("A comp range names an uncaptured clip"))?;
        let end = source.start.saturating_add(source.length);
        if segment.start >= segment.end
            || segment.start < source.start
            || segment.end > end
            || segment.end > MAX_SONG_TICKS
            || index > 0 && segments[index - 1].end > segment.start
        {
            return Err(CommandError::invalid(
                "Comp ranges must not overlap and must fit inside their source clips",
            ));
        }
        let mut clip = (*source).clone();
        clip.start = segment.start;
        clip.length = segment.end - segment.start;
        clip.offset = source
            .offset
            .checked_add(segment.start - source.start)
            .filter(|offset| *offset <= MAX_SONG_TICKS)
            .ok_or_else(|| CommandError::invalid("Comp source offset is outside the timeline"))?;
        clip.muted = false;
        if let ClipContent::Audio {
            fade_in, fade_out, ..
        } = &mut clip.content
        {
            // Retain only the part of an original fade visible in this range.
            *fade_in = fade_in
                .saturating_sub(segment.start - source.start)
                .min(clip.length);
            *fade_out = fade_out.saturating_sub(end - segment.end).min(clip.length);
        }
        prepared.push(clip);
    }
    for index in 1..prepared.len() {
        let before = segments[index - 1];
        let after = segments[index];
        if before.end != after.start || before.clip == after.clip {
            continue;
        }
        let first = captured[&before.clip];
        let second = captured[&after.clip];
        let width = fade_ticks
            .min((before.end - before.start) / 2)
            .min((after.end - after.start) / 2);
        let left = (width / 2)
            .min(after.start - second.start)
            .min(prepared[index].offset);
        let right = (width - width / 2).min(first.start.saturating_add(first.length) - before.end);
        let overlap = left + right;
        if overlap == 0 {
            continue;
        }
        prepared[index - 1].length += right;
        prepared[index].start -= left;
        prepared[index].offset -= left;
        prepared[index].length += left;
        if let ClipContent::Audio { fade_out, .. } = &mut prepared[index - 1].content {
            *fade_out = overlap;
        }
        if let ClipContent::Audio { fade_in, .. } = &mut prepared[index].content {
            *fade_in = overlap;
        }
    }
    Ok(prepared)
}

/// Every input lane receives the same pass/range choices and overlap geometry.
pub(crate) fn prepare_group(
    project: &Project,
    expected: &crate::AudioTakeGroup,
    sources: &[Clip],
    ranges: &[crate::TakeCompRange],
    fade_ticks: u32,
) -> Result<Vec<(String, Vec<Clip>)>, CommandError> {
    if project
        .playlist
        .take_groups
        .iter()
        .find(|group| group.id == expected.id)
        != Some(expected)
    {
        return Err(CommandError::invalid(
            "The take group changed; reopen the comp editor",
        ));
    }
    crate::take_groups::check_group(project, expected).map_err(CommandError::invalid)?;
    if expected.lanes.is_empty() || ranges.is_empty() || ranges.len() > 2048 {
        return Err(CommandError::invalid(
            "Choose 1–2048 ranges from retained input lanes",
        ));
    }
    let captured: HashMap<_, _> = sources.iter().map(|clip| (clip.id, clip)).collect();
    let total: usize = expected.lanes.iter().map(|lane| lane.takes.len()).sum();
    if sources.len() != total || captured.len() != total {
        return Err(CommandError::invalid(
            "Capture every retained source exactly once",
        ));
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| (range.start, range.end));
    let mut ranges: Vec<crate::TakeCompRange> = Vec::with_capacity(sorted.len());
    for range in sorted {
        if let Some(previous) = ranges.last_mut()
            && previous.pass == range.pass
            && previous.end == range.start
        {
            previous.end = range.end;
        } else {
            ranges.push(range);
        }
    }
    let mut output = Vec::with_capacity(expected.lanes.len());
    let mut lane_sources = Vec::with_capacity(expected.lanes.len());
    for lane in &expected.lanes {
        let clips = lane
            .takes
            .iter()
            .map(|take| {
                captured.get(&take.clip).copied().cloned().ok_or_else(|| {
                    CommandError::invalid("A retained input source was not captured")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let selected = ranges
            .iter()
            .map(|range| {
                let take = lane
                    .takes
                    .iter()
                    .find(|take| take.pass == range.pass)
                    .ok_or_else(|| {
                        CommandError::invalid("A chosen pass is unavailable on an input lane")
                    })?;
                Ok(AudioCompSegment {
                    clip: take.clip,
                    start: range.start,
                    end: range.end,
                })
            })
            .collect::<Result<Vec<_>, CommandError>>()?;
        let prepared = prepare(project, &clips, &selected, 0)?;
        output.push((lane.name.clone(), prepared));
        lane_sources.push(selected);
    }
    for index in 1..ranges.len() {
        let before = ranges[index - 1];
        let after = ranges[index];
        if before.end != after.start || before.pass == after.pass {
            continue;
        }
        let width = fade_ticks
            .min((before.end - before.start) / 2)
            .min((after.end - after.start) / 2);
        let mut left = width / 2;
        let mut right = width - width / 2;
        for (lane, selected) in lane_sources.iter().enumerate() {
            let first = captured[&selected[index - 1].clip];
            let second = captured[&selected[index].clip];
            left = left
                .min(after.start - second.start)
                .min(output[lane].1[index].offset);
            right = right.min(first.start.saturating_add(first.length) - before.end);
        }
        let overlap = left + right;
        for (_, clips) in &mut output {
            clips[index - 1].length += right;
            clips[index].start -= left;
            clips[index].offset -= left;
            clips[index].length += left;
            if overlap > 0 {
                if let ClipContent::Audio { fade_out, .. } = &mut clips[index - 1].content {
                    *fade_out = overlap;
                }
                if let ClipContent::Audio { fade_in, .. } = &mut clips[index].content {
                    *fade_in = overlap;
                }
            }
        }
    }
    Ok(output)
}
