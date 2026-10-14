//! Document-owned cue annotations. Unknown RIFF notes remain verbatim; known
//! frame-bearing annotations follow the same coordinate map as their region.
use windfall_codec::{WavCue, WavLoop, WavLoopKind, WavMetadata, WavNote, WavSlice, WavTempo};
use windfall_project::AudioRegion;

pub(crate) fn read(metadata: &WavMetadata, frames: u32) -> Result<Vec<AudioRegion>, String> {
    metadata
        .validate(Some(u64::from(frames)))
        .map_err(|e| e.to_string())?;
    let mut regions = Vec::new();
    let mut global_notes: Vec<String> = metadata
        .notes
        .iter()
        .filter(|n| n.text.starts_with("WindfallBeats/1 "))
        .map(|n| n.text.clone())
        .collect();
    for cue in &metadata.cues {
        let end = metadata
            .slices
            .iter()
            .find(|s| s.cue_id == cue.id)
            .map_or(cue.frame, |s| cue.frame + s.length);
        let mut region = AudioRegion {
            name: if cue.label.trim().is_empty() {
                format!("Cue {}", cue.id)
            } else {
                cue.label.clone()
            },
            start_frame: cue.frame,
            end_frame: end,
            notes: metadata
                .notes
                .iter()
                .filter(|n| n.cue_id == cue.id && !n.text.starts_with("WindfallBeats/1 "))
                .map(|n| n.text.clone())
                .collect(),
        };
        if let Some(l) = metadata.loops.iter().find(|l| l.cue_id == cue.id) {
            if l.start != cue.frame {
                return Err("WAV loop and its cue have different starts.".into());
            }
            region.end_frame = l.end;
            region.notes.push(format!(
                "WindfallLoop/1 {} {}",
                match l.kind {
                    WavLoopKind::Forward => "forward",
                    WavLoopKind::Alternating => "alternating",
                    WavLoopKind::Backward => "backward",
                },
                l.play_count
            ));
        }
        for note in &region.notes {
            if let Some(text) = note.strip_prefix("WindfallNavigation/1 ") {
                let fields: Vec<_> = text.split_whitespace().collect();
                if fields.len() == 2 && ["loop", "skip"].contains(&fields[0]) {
                    region.end_frame = fields[1]
                        .parse()
                        .map_err(|_| "Invalid navigation end frame.")?;
                }
            }
        }
        let global_anchor = region.name == "Source metadata"
            && region.notes.is_empty()
            && region.start_frame == region.end_frame
            && metadata
                .notes
                .iter()
                .any(|n| n.cue_id == cue.id && n.text.starts_with("WindfallBeats/1 "));
        if !global_anchor {
            regions.push(region);
        }
    }
    // smpl loops need not have a corresponding cue in third-party WAVs.
    for l in metadata
        .loops
        .iter()
        .filter(|l| !metadata.cues.iter().any(|c| c.id == l.cue_id))
    {
        regions.push(AudioRegion {
            name: format!("Loop {}", l.cue_id),
            start_frame: l.start,
            end_frame: l.end,
            notes: vec![format!(
                "WindfallLoop/1 {} {}",
                match l.kind {
                    WavLoopKind::Forward => "forward",
                    WavLoopKind::Alternating => "alternating",
                    WavLoopKind::Backward => "backward",
                },
                l.play_count
            )],
        });
    }
    if metadata.tempo.is_some() || metadata.root_key.is_some() || !global_notes.is_empty() {
        let notes = &mut global_notes;
        if let Some(t) = metadata.tempo {
            notes.push(format!(
                "WindfallTempo/1 {} {} {} {} {} {}",
                t.bpm,
                t.beats,
                t.numerator,
                t.denominator,
                t.root_key.map_or(-1, i16::from),
                u8::from(t.one_shot)
            ));
        }
        if let Some(key) = metadata.root_key {
            notes.push(format!("WindfallRoot/1 {key}"));
        }
        regions.push(AudioRegion {
            name: "Source metadata".into(),
            start_frame: 0,
            end_frame: frames,
            notes: global_notes,
        });
    }
    write(&regions, frames)?;
    Ok(regions)
}

pub(crate) fn write(regions: &[AudioRegion], frames: u32) -> Result<WavMetadata, String> {
    windfall_project::model::check_audio_regions(regions)?;
    let mut metadata = WavMetadata::default();
    let mut source_beats = None;
    for (index, region) in regions.iter().enumerate() {
        if region.end_frame > frames {
            return Err("An annotation extends beyond the audio.".into());
        }
        for prefix in [
            "WindfallNavigation/1 ",
            "WindfallLoop/1 ",
            "WindfallSlice/1 ",
        ] {
            if region
                .notes
                .iter()
                .filter(|note| note.starts_with(prefix))
                .count()
                > 1
            {
                return Err(windfall_ipc::ui_message(
                    "audioEdit.repeatedAnnotation",
                    serde_json::json!({"prefix": prefix}),
                ));
            }
        }
        let id = index as u32 + 1;
        let global = !region.notes.is_empty()
            && region.notes.iter().all(|n| {
                n.starts_with("WindfallTempo/1 ")
                    || n.starts_with("WindfallRoot/1 ")
                    || n.starts_with("WindfallBeats/1 ")
            });
        if !global
            || region
                .notes
                .iter()
                .any(|n| n.starts_with("WindfallBeats/1 "))
        {
            metadata.cues.push(WavCue {
                id,
                frame: region.start_frame,
                label: region.name.clone(),
            });
        }
        if !global && region.end_frame > region.start_frame {
            metadata.slices.push(WavSlice {
                cue_id: id,
                length: region.end_frame - region.start_frame,
            });
        }
        for note in &region.notes {
            let fields: Vec<_> = note.split_whitespace().collect();
            let number = |at: usize| -> Result<f64, String> {
                let value: f64 = fields
                    .get(at)
                    .ok_or("Missing annotation field")?
                    .parse()
                    .map_err(|_| "Invalid annotation number")?;
                if !value.is_finite() {
                    return Err("Nonfinite annotation number".into());
                }
                Ok(value)
            };
            match fields.first().copied() {
                Some("WindfallNavigation/1") => {
                    let valid = (fields.len() == 2 && fields[1] == "pause")
                        || (fields.len() == 3
                            && ["loop", "skip"].contains(&fields[1])
                            && fields[2].parse::<u32>().is_ok_and(|end| {
                                end == region.end_frame && end > region.start_frame
                            }));
                    if !valid {
                        return Err("Malformed navigation annotation or region endpoints.".into());
                    }
                }
                Some("WindfallLoop/1") => {
                    if fields.len() != 3 || region.start_frame >= region.end_frame {
                        return Err("Malformed loop annotation.".into());
                    }
                    let kind = match fields[1] {
                        "forward" => WavLoopKind::Forward,
                        "alternating" => WavLoopKind::Alternating,
                        "backward" => WavLoopKind::Backward,
                        _ => return Err("Invalid loop kind.".into()),
                    };
                    metadata.loops.push(WavLoop {
                        cue_id: id,
                        start: region.start_frame,
                        end: region.end_frame,
                        kind,
                        play_count: fields[2].parse().map_err(|_| "Invalid loop count")?,
                    });
                    continue; // standard smpl carries this; avoid duplicating on reimport
                }
                Some("WindfallTempo/1") => {
                    if fields.len() != 7 || metadata.tempo.is_some() {
                        return Err("Malformed or repeated source tempo.".into());
                    }
                    let key: i16 = fields[5].parse().map_err(|_| "Invalid tempo root")?;
                    if !(-1..=127).contains(&key) || !["0", "1"].contains(&fields[6]) {
                        return Err("Invalid tempo flags.".into());
                    }
                    metadata.tempo = Some(WavTempo {
                        bpm: number(1)? as f32,
                        beats: fields[2].parse().map_err(|_| "Invalid beat count")?,
                        numerator: fields[3].parse().map_err(|_| "Invalid meter numerator")?,
                        denominator: fields[4].parse().map_err(|_| "Invalid meter denominator")?,
                        root_key: (key >= 0).then_some(key as u8),
                        one_shot: fields[6] == "1",
                    });
                    continue;
                }
                Some("WindfallRoot/1") => {
                    if fields.len() != 2 || metadata.root_key.is_some() {
                        return Err("Malformed or repeated root key.".into());
                    }
                    metadata.root_key = Some(fields[1].parse().map_err(|_| "Invalid root key")?);
                    continue;
                }
                Some("WindfallBeats/1") => {
                    if source_beats.is_some()
                        || fields.len() != 3
                        || !(10.0..=999.0).contains(&number(1)?)
                        || !(0.0..=1_000_000.0).contains(&number(2)?)
                    {
                        return Err("Invalid source BPM or beat count.".into());
                    }
                    source_beats = Some((number(1)?, number(2)?));
                }
                Some("WindfallSlice/1")
                    if (fields.len() != 7
                        || region.start_frame >= region.end_frame
                        || !(0.0..=2.0).contains(&number(1)?)
                        || !(-48.0..=48.0).contains(&number(2)?)
                        || !(0.0..=1000.0).contains(&number(3)?)
                        || !(1.0..=1000.0).contains(&number(4)?)
                        || !["0", "1"].contains(&fields[5])
                        || !["0", "1"].contains(&fields[6])) =>
                {
                    return Err("Invalid slice attributes.".into());
                }
                _ => {}
            }
            metadata.notes.push(WavNote {
                cue_id: id,
                text: note.clone(),
            });
        }
    }
    if let Some((bpm, beats)) = source_beats {
        let tempo = metadata.tempo.get_or_insert(WavTempo {
            bpm: bpm as f32,
            beats: 0,
            numerator: 4,
            denominator: 4,
            root_key: None,
            one_shot: false,
        });
        tempo.bpm = bpm as f32;
        tempo.beats = if beats.fract() == 0.0 {
            beats as u32
        } else {
            0
        };
    }
    metadata
        .validate(Some(u64::from(frames)))
        .map_err(|e| e.to_string())?;
    Ok(metadata)
}

pub(crate) fn mapped(region: &AudioRegion, map: impl Fn(u32) -> u32) -> AudioRegion {
    let a = map(region.start_frame);
    let b = map(region.end_frame);
    let mut result = region.clone();
    result.start_frame = a.min(b);
    result.end_frame = a.max(b);
    for note in &mut result.notes {
        if let Some(text) = note.strip_prefix("WindfallNavigation/1 ")
            && let Some((kind, _)) = text.split_once(' ')
        {
            *note = format!("WindfallNavigation/1 {kind} {}", result.end_frame);
        }
        if a == b
            && (note.starts_with("WindfallLoop/1 ")
                || note.starts_with("WindfallSlice/1 ")
                || note.starts_with("WindfallNavigation/1 loop ")
                || note.starts_with("WindfallNavigation/1 skip "))
        {
            // A collapsed region is a named marker, never an invalid action.
            *note = format!("Collapsed annotation: {note}");
        }
    }
    result
}

/// Cropping reduces the musical extent; stretching changes frames, not beats.
pub(crate) fn crop_beats(region: &mut AudioRegion, fraction: f64) {
    if fraction >= 1.0 {
        return;
    }
    let mut added = None;
    let has_beats = region
        .notes
        .iter()
        .any(|n| n.starts_with("WindfallBeats/1 "));
    for note in &mut region.notes {
        if let Some(text) = note.strip_prefix("WindfallBeats/1 ")
            && let Some((bpm, beats)) = text.split_once(' ')
            && let Ok(beats) = beats.parse::<f64>()
        {
            *note = format!("WindfallBeats/1 {bpm} {}", beats * fraction);
        } else if let Some(text) = note.strip_prefix("WindfallTempo/1 ") {
            let mut fields: Vec<_> = text.split_whitespace().map(str::to_owned).collect();
            if fields.len() == 6 {
                if !has_beats
                    && let Ok(beats) = fields[1].parse::<f64>()
                    && beats > 0.0
                {
                    added = Some(format!(
                        "WindfallBeats/1 {} {}",
                        fields[0],
                        beats * fraction
                    ));
                }
                fields[1] = "0".into();
                *note = format!("WindfallTempo/1 {}", fields.join(" "));
            }
        }
    }
    if let Some(note) = added {
        region.notes.push(note);
    }
}

impl super::Session {
    /// Export the actual editor view/selection and its transformed annotations.
    /// Destination publication shares the token/source/edit guard of WavPack.
    pub fn audio_editor_save_wav(
        &self,
        token: u32,
        start: u32,
        end: u32,
        path: String,
    ) -> Result<(), String> {
        use super::{Output, SERIAL, STALE, edited_regions, view_regions};
        use std::sync::atomic::Ordering;
        let editor = crate::sync::lock(&self.inner.audio_editor);
        let prepared = editor
            .prepared
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or("This audio editor has expired.")?;
        if !prepared.valid(&self.state()) {
            return Err(STALE.into());
        }
        if start >= end || end as usize > prepared.view.frames() {
            return Err("Choose a nonempty editor selection.".into());
        }
        let target = std::path::PathBuf::from(path);
        if !target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        {
            return Err("A WAV file must end in .wav.".into());
        }
        let name = target.file_name().ok_or("Choose a filename")?;
        let mut staged_name = std::ffi::OsString::from(".");
        staged_name.push(name);
        staged_name.push(format!(
            ".{}-{}.wav",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let staged_path = target.with_file_name(staged_name);
        drop(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged_path)
                .map_err(|e| e.to_string())?,
        );
        let staged = Output(staged_path); // cleanup owns only a successful reservation
        let request = windfall_ipc::AudioEditRequest {
            token,
            operation: windfall_ipc::AudioEditOperation::Extract,
            start_frame: start,
            end_frame: end,
        };
        let regions = edited_regions(
            &view_regions(prepared, &self.state()),
            request,
            prepared.view.frames(),
            (end - start) as usize,
            None,
        );
        let metadata = write(&regions, end - start)?;
        let mut writer = windfall_codec::WavWriter::create_with_metadata(
            &staged.0,
            prepared.view.sample_rate(),
            prepared.view.channels(),
            windfall_codec::WavSampleFormat::Float32,
            &windfall_codec::AudioTags::default(),
            &metadata,
        )
        .map_err(|e| e.to_string())?;
        let channels = usize::from(prepared.view.channels());
        writer
            .write(&prepared.view.samples()[start as usize * channels..end as usize * channels])
            .map_err(|e| e.to_string())?;
        writer.finalize().map_err(|e| e.to_string())?;
        #[cfg(test)]
        self.pause("audio-editor:wav-staged");
        let _recording = self.recording_idle()?;
        let state = self.state();
        if !prepared.valid(&state) {
            return Err(STALE.into());
        }
        std::fs::rename(&staged.0, target).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn audio_editor_import_timeline(
        &self,
        token: u32,
    ) -> Result<windfall_project::DispatchResult, String> {
        use windfall_project::{Command, MarkerKind};
        let mut editor = crate::sync::lock(&self.inner.audio_editor);
        let prepared = editor
            .prepared
            .as_ref()
            .filter(|p| p.token == token)
            .ok_or("This audio editor has expired.")?;
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !prepared.valid(&state) {
                return Err(super::STALE.into());
            }
            let mut commands = Vec::new();
            for region in super::view_regions(prepared, &state) {
                if region.name == "Source metadata"
                    && !region.notes.is_empty()
                    && region.notes.iter().all(|n| {
                        n.starts_with("WindfallTempo/")
                            || n.starts_with("WindfallRoot/")
                            || n.starts_with("WindfallBeats/")
                    })
                {
                    continue;
                }
                let tick = prepared.clock.tick_after(
                    prepared.clip.start,
                    f64::from(region.start_frame) / f64::from(prepared.view.sample_rate()),
                )?;
                let end = prepared.clock.tick_after(
                    prepared.clip.start,
                    f64::from(region.end_frame) / f64::from(prepared.view.sample_rate()),
                )?;
                let mut kind = MarkerKind::Named;
                for note in &region.notes {
                    if note == "WindfallNavigation/1 pause" {
                        kind = MarkerKind::Pause;
                    }
                    if note.starts_with("WindfallNavigation/1 loop ")
                        || note == "WindfallLoop/1 forward 0"
                    {
                        kind = MarkerKind::Loop { end };
                    }
                    if note.starts_with("WindfallNavigation/1 skip ") {
                        kind = MarkerKind::Skip { end };
                    }
                }
                commands.push(Command::AddTimelineMarker {
                    tick,
                    name: region.name.clone(),
                    kind,
                });
                // Named regions have two visible timeline boundaries. Navigation
                // regions have an action-aware endpoint in MarkerKind already.
                if kind == MarkerKind::Named && end > tick {
                    commands.push(Command::AddTimelineMarker {
                        tick: end,
                        name: format!("{} end", region.name),
                        kind: MarkerKind::Named,
                    });
                }
            }
            if commands.is_empty() {
                return Err("There are no visible WAV markers to import.".into());
            }
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some("Import WAV timeline markers".into()),
                    commands,
                },
                None,
                Vec::new(),
            )?
        };
        let result = self.finish_sample_edit(ticket)?;
        editor.prepared = None;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_frame_maps_preserve_unknown_notes_loop_types_counts_and_fractional_beats() {
        let region = AudioRegion {
            name: "Phrase".into(),
            start_frame: 10,
            end_frame: 30,
            notes: vec![
                "A note\nwith two lines".into(),
                "WindfallLoop/1 alternating 3".into(),
                "WindfallNavigation/1 skip 30".into(),
            ],
        };
        let mapped = mapped(&region, |frame| 100 - frame);
        assert_eq!((mapped.start_frame, mapped.end_frame), (70, 90));
        assert_eq!(mapped.notes[0], "A note\nwith two lines");
        assert_eq!(mapped.notes[2], "WindfallNavigation/1 skip 90");
        let metadata = write(&[mapped], 100).unwrap();
        assert_eq!(metadata.loops[0].kind, WavLoopKind::Alternating);
        assert_eq!(metadata.loops[0].play_count, 3);
        let regions = read(&metadata, 100).unwrap();
        assert_eq!(regions[0].notes[0], "A note\nwith two lines");
        assert!(
            regions[0]
                .notes
                .contains(&"WindfallLoop/1 alternating 3".into())
        );
        let mut beats = AudioRegion {
            name: "Source metadata".into(),
            start_frame: 0,
            end_frame: 100,
            notes: vec![
                "WindfallTempo/1 120 4 4 4 -1 0".into(),
                "WindfallRoot/1 60".into(),
            ],
        };
        crop_beats(&mut beats, 0.375);
        assert!(beats.notes.contains(&"WindfallBeats/1 120 1.5".into()));
        let metadata = write(&[beats], 100).unwrap();
        assert_eq!(metadata.tempo.unwrap().beats, 0);
        assert_eq!(metadata.root_key, Some(60));
    }

    #[test]
    fn malformed_annotations_and_overflow_are_explicitly_refused() {
        for note in [
            "WindfallNavigation/1 skip 0",
            "WindfallLoop/1 unknown 0",
            "WindfallRoot/1 128",
            "WindfallTempo/1 120 4 0 4 -1 0",
            "WindfallBeats/1 120 NaN",
            "WindfallSlice/1 1 0 1 4 0 2",
        ] {
            assert!(
                write(
                    &[AudioRegion {
                        name: "Cue".into(),
                        start_frame: 0,
                        end_frame: 10,
                        notes: vec![note.into()]
                    }],
                    10
                )
                .is_err(),
                "{note}"
            );
        }
        let metadata = WavMetadata {
            cues: (0..257)
                .map(|id| WavCue {
                    id,
                    frame: 0,
                    label: "Cue".into(),
                })
                .collect(),
            ..Default::default()
        };
        assert!(read(&metadata, 10).is_err());
    }
}
