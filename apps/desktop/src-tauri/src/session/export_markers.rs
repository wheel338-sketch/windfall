//! Metadata is captured from the same immutable project as the render job.
use windfall_codec::{WavCue, WavLoop, WavLoopKind, WavMetadata, WavNote, WavTempo};
use windfall_engine::{SamplePool, SongFrameMap};
use windfall_project::{MarkerKind, PatternId, Project, TickRange};

pub(super) fn song_metadata(
    project: &Project,
    pool: &SamplePool,
    rate: u32,
    region: Option<TickRange>,
) -> Result<WavMetadata, String> {
    let map = SongFrameMap::new(project, pool, rate);
    let first = region.map_or(0, |r| r.start);
    let last = region.map_or(map.end_tick, |r| r.end);
    // Export uses play_passes, which disables interactive navigation. Cues
    // therefore follow the finite linear render; note chunks preserve actions
    // for an importer without falsely shifting physical audio positions.
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
        match marker.kind {
            MarkerKind::Skip { end } | MarkerKind::Loop { end } if end <= last => {
                metadata.notes.push(WavNote {
                    cue_id: marker.id.0,
                    text: format!(
                        "WindfallNavigation/1 {} {}",
                        if matches!(marker.kind, MarkerKind::Skip { .. }) {
                            "skip"
                        } else {
                            "loop"
                        },
                        frame(end)?
                    ),
                });
            }
            MarkerKind::Pause => metadata.notes.push(WavNote {
                cue_id: marker.id.0,
                text: "WindfallNavigation/1 pause".into(),
            }),
            _ => {}
        }
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
    if !map.has_tempo_map() && project.playlist.timeline.meters.is_empty() {
        if metadata.cues.is_empty() {
            metadata.cues.push(WavCue {
                id: 0,
                frame: 0,
                label: project.settings.name.clone(),
            });
        }
        metadata.notes.push(WavNote {
            cue_id: metadata.cues[0].id,
            text: format!(
                "WindfallBeats/1 {} {}",
                project.settings.tempo_bpm,
                f64::from(last - first) / f64::from(windfall_project::PPQ)
            ),
        });
        let signature = project.settings.time_signature;
        metadata.tempo = Some(WavTempo {
            bpm: project.settings.tempo_bpm as f32,
            beats: if (last - first).is_multiple_of(windfall_project::PPQ) {
                (last - first) / windfall_project::PPQ
            } else {
                0
            },
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

/// Pattern rendering uses the stored tempo and channel timing, including swing,
/// gate and shift. IDs here are export-local, so repeated notes get unique cues.
pub(super) fn pattern_metadata(
    project: &Project,
    pattern: PatternId,
    rate: u32,
    passes: u32,
) -> Result<WavMetadata, String> {
    let pattern = project
        .pattern(pattern)
        .ok_or("The export pattern no longer exists.")?;
    let length = pattern.length_ticks();
    let per_tick = windfall_core::samples_per_tick(project.settings.tempo_bpm, f64::from(rate));
    let frame = |tick: f64| -> Result<u32, String> {
        // Match sequencer::Clock's 1e-6-frame tolerance at exact boundaries.
        let value = (tick * per_tick - 1e-6).ceil().max(0.0);
        if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
            return Err("A WAV note marker exceeds the 32-bit frame range.".into());
        }
        Ok(value as u32)
    };
    let mut metadata = WavMetadata::default();
    let mut cue = |tick: f64, label: String| -> Result<(), String> {
        if metadata.cues.len() >= windfall_codec::MAX_WAV_MARKERS {
            return Err("The pattern has more than 4096 WAV markers. Export fewer loops.".into());
        }
        metadata.cues.push(WavCue {
            id: metadata.cues.len() as u32 + 1,
            frame: frame(tick)?,
            label,
        });
        Ok(())
    };
    for pass in 0..passes {
        let origin = f64::from(pass) * f64::from(length);
        for marker in &pattern.timeline.markers {
            if marker.tick < length {
                cue(origin + f64::from(marker.tick), marker.name.clone())?;
            }
        }
        for lane in &pattern.lanes {
            let solo = project.channels.iter().any(|c| c.solo);
            let Some(channel) = project
                .channel(lane.channel)
                .filter(|c| !c.muted && (!solo || c.solo))
            else {
                continue;
            };
            for note in &lane.notes {
                if let Some((tick, _)) = channel.timing.place(
                    note.start,
                    note.length,
                    length,
                    f64::from(project.settings.swing),
                ) {
                    cue(
                        origin + tick,
                        format!("{}: MIDI {}", channel.name, note.key),
                    )?;
                }
            }
        }
    }
    let signature = pattern.effective_signature(project.settings.time_signature);
    if pattern.timeline.meters.is_empty() {
        if metadata.cues.is_empty() {
            metadata.cues.push(WavCue {
                id: 1,
                frame: 0,
                label: pattern.name.clone(),
            });
        }
        metadata.notes.push(WavNote {
            cue_id: metadata.cues[0].id,
            text: format!(
                "WindfallBeats/1 {} {}",
                project.settings.tempo_bpm,
                f64::from(length) * f64::from(passes) / f64::from(windfall_project::PPQ)
            ),
        });
        metadata.tempo = Some(WavTempo {
            bpm: project.settings.tempo_bpm as f32,
            beats: if length.is_multiple_of(windfall_project::PPQ) {
                (length / windfall_project::PPQ)
                    .checked_mul(passes)
                    .ok_or("The pattern beat count exceeds the WAV metadata range.")?
            } else {
                0
            },
            numerator: u16::from(signature.numerator),
            denominator: u16::from(signature.denominator),
            root_key: None,
            one_shot: true,
        });
    }
    metadata
        .validate(Some(u64::from(frame(
            f64::from(length) * f64::from(passes),
        )?)))
        .map_err(|e| e.to_string())?;
    Ok(metadata)
}
#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::{Command, Document};
    #[test]
    fn navigation_actions_have_notes_on_the_finite_linear_render_grid() {
        let mut document = Document::new(Project::new("Original navigation fixture"));
        document
            .dispatch(
                Command::AddTimelineMarker {
                    tick: 960,
                    name: "Skip".into(),
                    kind: MarkerKind::Skip { end: 1920 },
                },
                None,
            )
            .unwrap();
        let metadata = song_metadata(
            document.project(),
            &SamplePool::new(),
            48000,
            Some(TickRange {
                start: 0,
                end: 3840,
            }),
        )
        .unwrap();
        assert_eq!(metadata.cues[0].frame, 24000);
        assert_eq!(metadata.notes[0].text, "WindfallNavigation/1 skip 48000");
        document
            .dispatch(
                Command::AddTimelineMarker {
                    tick: 2400,
                    name: "Pause".into(),
                    kind: MarkerKind::Pause,
                },
                None,
            )
            .unwrap();
        let metadata = song_metadata(
            document.project(),
            &SamplePool::new(),
            48000,
            Some(TickRange {
                start: 0,
                end: 3840,
            }),
        )
        .unwrap();
        assert_eq!(metadata.cues[1].frame, 60000);
        assert_eq!(metadata.notes[1].text, "WindfallNavigation/1 pause");
    }
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
