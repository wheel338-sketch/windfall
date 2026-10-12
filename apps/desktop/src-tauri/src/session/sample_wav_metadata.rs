//! Standard source regions plus documented Windfall v1 RIFF note attributes.
use windfall_codec::{WavCue, WavMetadata, WavNote, WavSlice, WavTempo};
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{MAX_ASSET_SLICES, SliceRegion};
use windfall_project::SamplePlayback;

pub(super) fn read(metadata: &WavMetadata, audio: &AudioBuffer) -> Result<SamplePlayback, String> {
    metadata
        .validate(Some(audio.frames() as u64))
        .map_err(|e| e.to_string())?;
    if metadata.slices.is_empty() || metadata.slices.len() > MAX_ASSET_SLICES || audio.frames() == 0
    {
        return Err("Choose a WAV with 1–128 authored slice regions (cue/ltxt).".into());
    }
    // Vector order is MIDI key order. Do not sort, merge overlapping slices or
    // fill gaps: all three would change the authored instrument.
    let mut slices = Vec::with_capacity(metadata.slices.len());
    for slice in &metadata.slices {
        let cue = metadata
            .cues
            .iter()
            .find(|cue| cue.id == slice.cue_id)
            .ok_or("A WAV slice has no cue point.")?;
        let mut region = SliceRegion {
            label: cue.label.clone(),
            start: (f64::from(cue.frame) / audio.frames() as f64) as f32,
            end: ((f64::from(cue.frame) + f64::from(slice.length)) / audio.frames() as f64) as f32,
            ..Default::default()
        };
        let mut attributes = false;
        for note in metadata.notes.iter().filter(|n| n.cue_id == cue.id) {
            if let Some(text) = note.text.strip_prefix("WindfallSlice/1 ") {
                if attributes {
                    return Err("Repeated slice attributes in WAV notes.".into());
                }
                attributes = true;
                let fields: Vec<_> = text.split_whitespace().collect();
                if fields.len() != 6 {
                    return Err("Malformed Windfall slice note.".into());
                }
                region.gain = fields[0].parse().map_err(|_| "Invalid slice gain.")?;
                region.pitch = fields[1].parse().map_err(|_| "Invalid slice pitch.")?;
                region.attack_ms = fields[2].parse().map_err(|_| "Invalid slice attack.")?;
                region.release_ms = fields[3].parse().map_err(|_| "Invalid slice release.")?;
                region.reverse = match fields[4] {
                    "0" => false,
                    "1" => true,
                    _ => return Err("Invalid slice reverse flag.".into()),
                };
                region.deck = fields[5].parse().map_err(|_| "Invalid slice deck.")?;
            } else if note.text.starts_with("WindfallSlice/") {
                return Err("Unsupported Windfall slice note version.".into());
            }
        }
        slices.push(region);
    }
    let mut bpm = metadata.tempo.map_or(120.0, |t| f64::from(t.bpm));
    let mut beats = metadata
        .tempo
        .filter(|t| t.beats != 0)
        .map_or(audio.duration_secs() * bpm / 60.0, |t| f64::from(t.beats));
    let mut has_beats = false;
    for note in &metadata.notes {
        if let Some(text) = note.text.strip_prefix("WindfallBeats/1 ") {
            let fields: Vec<_> = text.split_whitespace().collect();
            if has_beats || fields.len() != 2 {
                return Err("Repeated or malformed Windfall beat note.".into());
            }
            has_beats = true;
            bpm = fields[0].parse().map_err(|_| "Invalid source tempo.")?;
            beats = fields[1]
                .parse()
                .map_err(|_| "Invalid source beat count.")?;
        } else if note.text.starts_with("WindfallBeats/") {
            return Err("Unsupported Windfall beat note version.".into());
        }
    }
    let asset = SamplePlayback {
        slices,
        source_bpm: bpm,
        source_beats: beats,
        ..Default::default()
    };
    asset.validate()?;
    Ok(asset)
}

pub(super) fn write(asset: &SamplePlayback, audio: &AudioBuffer) -> Result<WavMetadata, String> {
    write_regions(asset, audio.frames(), 0, audio.frames(), audio.frames())
}

fn write_regions(
    asset: &SamplePlayback,
    a_frames: usize,
    b_offset: usize,
    b_frames: usize,
    total_frames: usize,
) -> Result<WavMetadata, String> {
    asset.validate()?;
    let frames =
        u32::try_from(total_frames).map_err(|_| "The source exceeds WAV marker frame limits.")?;
    if frames == 0 {
        return Err("Cannot export an empty source.".into());
    }
    let mut metadata = WavMetadata::default();
    for (index, slice) in asset.slices.iter().enumerate() {
        let (offset, count) = if slice.deck == 0 {
            (0, a_frames)
        } else {
            (b_offset, b_frames)
        };
        let start = offset as u32 + (f64::from(slice.start) * count as f64).round() as u32;
        let end = offset as u32 + (f64::from(slice.end) * count as f64).round() as u32;
        if start >= end {
            return Err("A slice is shorter than one source frame.".into());
        }
        let id = index as u32 + 1;
        metadata.cues.push(WavCue {
            id,
            frame: start,
            label: if slice.label.is_empty() {
                format!("Slice {}", index + 1)
            } else {
                slice.label.clone()
            },
        });
        metadata.slices.push(WavSlice {
            cue_id: id,
            length: end - start,
        });
        metadata.notes.push(WavNote {
            cue_id: id,
            text: format!(
                "WindfallSlice/1 {} {} {} {} {} {}",
                slice.gain,
                slice.pitch,
                slice.attack_ms,
                slice.release_ms,
                u8::from(slice.reverse),
                slice.deck
            ),
        });
    }
    metadata.notes.push(WavNote {
        cue_id: 1,
        text: format!(
            "WindfallBeats/1 {} {}",
            asset.source_bpm, asset.source_beats
        ),
    });
    // ACID carries tempo/meter for every file. Zero means its integer beat
    // count is unspecified; the versioned note carries the exact fraction.
    metadata.tempo = Some(WavTempo {
        bpm: asset.source_bpm as f32,
        beats: if asset.source_beats.fract() == 0.0 {
            asset.source_beats as u32
        } else {
            0
        },
        numerator: 4,
        denominator: 4,
        root_key: None,
        one_shot: false,
    });
    metadata.root_key = Some(60);
    metadata
        .validate(Some(u64::from(frames)))
        .map_err(|e| e.to_string())?;
    Ok(metadata)
}

/// One self-contained source WAV: A followed by B on A's sample grid. On
/// import both decks reference that owned WAV and keep their cue-frame regions.
/// A shared source is written once, so re-export never doubles its duration.
pub(super) fn sources(
    asset: &SamplePlayback,
    a: &AudioBuffer,
    b: Option<&AudioBuffer>,
) -> Result<(AudioBuffer, WavMetadata), String> {
    if !asset.slices.iter().any(|s| s.deck == 1) {
        return Ok((a.clone(), write(asset, a)?));
    }
    let b = b.ok_or("Source B is not loaded. Reload samples first.")?;
    if a.samples().as_ptr() == b.samples().as_ptr()
        && a.frames() == b.frames()
        && a.channels() == b.channels()
        && a.sample_rate() == b.sample_rate()
    {
        return Ok((a.clone(), write(asset, a)?));
    }
    let b_frames = (b.frames() as f64 * f64::from(a.sample_rate()) / f64::from(b.sample_rate()))
        .round() as usize;
    let total = a
        .frames()
        .checked_add(b_frames)
        .ok_or("Source frame count overflow.")?;
    if b_frames == 0 || total > windfall_dsp::granular::MAX_SOURCE_FRAMES {
        return Err("Combined sources exceed the 32 million frame budget or B is empty.".into());
    }
    let channels = a.channels().max(b.channels());
    let count = total
        .checked_mul(usize::from(channels))
        .ok_or("Source sample count overflow.")?;
    if count as u64 * 4 > windfall_codec::DEFAULT_MAX_DECODED_BYTES {
        return Err("Combined sources exceed the decoded audio budget.".into());
    }
    let mut pcm = Vec::with_capacity(count);
    for (source, frames) in [(a, a.frames()), (b, b_frames)] {
        if source.frames() == 0 {
            return Err("Cannot export an empty deck.".into());
        }
        let source_channels = usize::from(source.channels());
        for frame in 0..frames {
            let position =
                frame as f64 * f64::from(source.sample_rate()) / f64::from(a.sample_rate());
            let first = (position.floor() as usize).min(source.frames() - 1);
            let next = (first + 1).min(source.frames() - 1);
            let fraction = (position - position.floor()) as f32;
            for channel in 0..usize::from(channels) {
                let channel = if source_channels == 1 {
                    0
                } else {
                    channel.min(source_channels - 1)
                };
                let left = source.samples()[first * source_channels + channel];
                let right = source.samples()[next * source_channels + channel];
                pcm.push(left + (right - left) * fraction);
            }
        }
    }
    let metadata = write_regions(asset, a.frames(), a.frames(), b_frames, total)?;
    Ok((
        AudioBuffer::from_interleaved(a.sample_rate(), channels, pcm),
        metadata,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_sources_labels_sound_attributes_and_fractional_beats_survive_cue_frame_roundtrip() {
        let a = AudioBuffer::from_interleaved(48000, 1, vec![0.25; 8]);
        let b = AudioBuffer::from_interleaved(48000, 1, vec![-0.5; 4]);
        let asset = SamplePlayback {
            source_beats: 3.125,
            source_bpm: 97.5,
            slices: vec![
                SliceRegion {
                    start: 0.25,
                    end: 0.75,
                    gain: 0.5,
                    pitch: -12.0,
                    attack_ms: 3.0,
                    release_ms: 25.0,
                    reverse: true,
                    label: "Authored A".into(),
                    ..Default::default()
                },
                SliceRegion {
                    start: 0.0,
                    end: 0.5,
                    deck: 1,
                    gain: 1.5,
                    pitch: 7.0,
                    label: "Authored B".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let (audio, metadata) = sources(&asset, &a, Some(&b)).unwrap();
        assert_eq!(
            audio.samples(),
            &[
                0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, -0.5, -0.5, -0.5, -0.5
            ]
        );
        assert_eq!(
            metadata.cues.iter().map(|c| c.frame).collect::<Vec<_>>(),
            [2, 8]
        );
        assert_eq!(
            metadata.slices.iter().map(|s| s.length).collect::<Vec<_>>(),
            [4, 2]
        );
        assert_eq!(metadata.tempo.unwrap().beats, 0);
        assert_eq!(metadata.tempo.unwrap().bpm, 97.5);
        assert_eq!(metadata.root_key, Some(60));
        let restored = read(&metadata, &audio).unwrap();
        assert_eq!((restored.source_beats, restored.source_bpm), (3.125, 97.5));
        for (before, after) in asset.slices.iter().zip(&restored.slices) {
            assert_eq!(
                (
                    before.gain,
                    before.pitch,
                    before.reverse,
                    before.attack_ms,
                    before.release_ms,
                    before.deck,
                    &before.label
                ),
                (
                    after.gain,
                    after.pitch,
                    after.reverse,
                    after.attack_ms,
                    after.release_ms,
                    after.deck,
                    &after.label
                )
            );
        }
        let (again, again_metadata) = sources(&restored, &audio, Some(&audio)).unwrap();
        assert_eq!(again.samples(), audio.samples());
        assert_eq!(again_metadata, metadata);
        let mut hostile = metadata.clone();
        hostile.notes[0].text = "WindfallSlice/1 NaN 0 1 4 0 0".into();
        assert!(read(&hostile, &audio).is_err());
        let mut hostile = metadata;
        hostile.notes.push(hostile.notes[0].clone());
        assert!(read(&hostile, &audio).is_err());
    }
    #[test]
    fn different_source_rates_and_channels_have_measured_linear_resampling() {
        let a = AudioBuffer::from_interleaved(48000, 1, vec![0.25; 2]);
        let b = AudioBuffer::from_interleaved(24000, 2, vec![0.0, 1.0, 0.5, -1.0]);
        let asset = SamplePlayback {
            slices: vec![SliceRegion {
                deck: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        let (audio, metadata) = sources(&asset, &a, Some(&b)).unwrap();
        assert_eq!(
            (audio.sample_rate(), audio.channels(), audio.frames()),
            (48000, 2, 6)
        );
        assert_eq!(
            audio.samples(),
            &[
                0.25, 0.25, 0.25, 0.25, 0.0, 1.0, 0.25, 0.0, 0.5, -1.0, 0.5, -1.0
            ]
        );
        assert_eq!((metadata.cues[0].frame, metadata.slices[0].length), (2, 4));
    }
    #[test]
    fn hostile_missing_overflow_and_subframe_regions_are_refused_before_export() {
        let audio = AudioBuffer::from_interleaved(48000, 1, vec![0.0; 8]);
        for (frame, length) in [(0, 0), (7, 2), (u32::MAX, 1)] {
            let metadata = WavMetadata {
                cues: vec![WavCue {
                    id: 1,
                    frame,
                    label: "Original".into(),
                }],
                slices: vec![WavSlice { cue_id: 1, length }],
                ..Default::default()
            };
            assert!(read(&metadata, &audio).is_err());
        }
        let missing = WavMetadata {
            slices: vec![WavSlice {
                cue_id: 2,
                length: 1,
            }],
            ..Default::default()
        };
        assert!(read(&missing, &audio).is_err());
        let tiny = SamplePlayback {
            slices: vec![SliceRegion {
                start: 0.0,
                end: 0.01,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(write(&tiny, &audio).is_err());
        let second_deck = SamplePlayback {
            slices: vec![SliceRegion {
                deck: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(sources(&second_deck, &audio, None).is_err());
    }
}
