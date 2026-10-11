//! Metadata is captured from the same immutable project as the render job.
use windfall_codec::{WavCue, WavLoop, WavLoopKind, WavMetadata, WavTempo};
use windfall_engine::{SamplePool, SongFrameMap};
use windfall_project::{MarkerKind, Project, TickRange};

pub(super) fn song_metadata(
    project: &Project,
    pool: &SamplePool,
    rate: u32,
    region: Option<TickRange>,
) -> Result<WavMetadata, String> {
    let map = SongFrameMap::new(project, pool, rate);
    let first = region.map_or(0, |r| r.start);
    let last = region.map_or(map.end_tick, |r| r.end);
    if project.playlist.timeline.markers.iter().any(|marker| {
        marker.tick >= first
            && marker.tick < last
            && matches!(marker.kind, MarkerKind::Skip { .. } | MarkerKind::Pause)
    }) {
        return Err("WAV marker export cannot yet map skip or pause navigation to rendered frames. Export a region without those actions.".to_owned());
    }
    let origin = map.frame(first);
    let frame = |tick| {
        u32::try_from(map.frame(tick).saturating_sub(origin))
            .map_err(|_| "A WAV marker position exceeds the 32-bit frame range.".to_owned())
    };
    let mut metadata = WavMetadata::default();
    for marker in &project.playlist.timeline.markers {
        if marker.tick < first || marker.tick > last {
            continue;
        }
        metadata.cues.push(WavCue {
            id: marker.id.0,
            frame: frame(marker.tick)?,
            label: marker.name.clone(),
        });
        if let MarkerKind::Loop { end } = marker.kind
            && end <= last
        {
            let start = frame(marker.tick)?;
            let end = frame(end)?;
            if start < end {
                metadata.loops.push(WavLoop {
                    cue_id: marker.id.0,
                    start,
                    end,
                    kind: WavLoopKind::Forward,
                    play_count: 0,
                });
            }
        }
    }
    // ACID has one tempo/meter, so it cannot describe an automated tempo or
    // changing meter honestly. Cue positions still follow the render clock.
    if !map.has_tempo_map()
        && project.playlist.timeline.meters.is_empty()
        && (last - first).is_multiple_of(windfall_project::PPQ)
    {
        let signature = project.settings.time_signature;
        metadata.tempo = Some(WavTempo {
            bpm: project.settings.tempo_bpm as f32,
            beats: (last - first) / windfall_project::PPQ,
            numerator: u16::from(signature.numerator),
            denominator: u16::from(signature.denominator),
            root_key: None,
            one_shot: metadata.loops.is_empty(),
        });
    }
    metadata
        .validate(Some(map.frame(last).saturating_sub(origin)))
        .map_err(|e| e.to_string())?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::{Command, Document};
    #[test]
    fn loop_metadata_uses_render_frames_and_region_origin() {
        let mut document = Document::new(Project::new("Original marker fixture"));
        document
            .dispatch(
                Command::AddTimelineMarker {
                    tick: 960,
                    name: "Own loop".into(),
                    kind: MarkerKind::Loop { end: 2880 },
                },
                None,
            )
            .unwrap();
        let metadata = song_metadata(
            document.project(),
            &SamplePool::new(),
            48000,
            Some(TickRange {
                start: 960,
                end: 3840,
            }),
        )
        .unwrap();
        assert_eq!(metadata.cues[0].frame, 0);
        assert_eq!((metadata.loops[0].start, metadata.loops[0].end), (0, 48000));
        assert_eq!(metadata.tempo.unwrap().beats, 3);
    }
}
