//! Authored source regions, not detector results or processed slice audio.
use windfall_codec::{WavCue, WavMetadata, WavSlice, WavTempo};
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
        slices.push(SliceRegion {
            start: (f64::from(cue.frame) / audio.frames() as f64) as f32,
            end: ((f64::from(cue.frame) + f64::from(slice.length)) / audio.frames() as f64) as f32,
            ..Default::default()
        });
    }
    let bpm = metadata.tempo.map_or(120.0, |t| f64::from(t.bpm));
    let beats = metadata
        .tempo
        .filter(|t| t.beats != 0)
        .map_or(audio.duration_secs() * bpm / 60.0, |t| f64::from(t.beats));
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
    asset.validate()?;
    let frames =
        u32::try_from(audio.frames()).map_err(|_| "The source exceeds WAV marker frame limits.")?;
    if frames == 0 || asset.slices.iter().any(|s| s.deck != 0) {
        return Err("Export source A slices only; a second deck needs its own source file.".into());
    }
    let mut metadata = WavMetadata::default();
    for (index, slice) in asset.slices.iter().enumerate() {
        let start = (f64::from(slice.start) * f64::from(frames)).round() as u32;
        let end = (f64::from(slice.end) * f64::from(frames)).round() as u32;
        if start >= end {
            return Err("A slice is shorter than one source frame.".into());
        }
        let id = index as u32 + 1;
        metadata.cues.push(WavCue {
            id,
            frame: start,
            label: format!("Slice {}", index + 1),
        });
        metadata.slices.push(WavSlice {
            cue_id: id,
            length: end - start,
        });
    }
    // ACID's integer beats cannot honestly represent a fractional source length.
    if asset.source_beats.fract() == 0.0 {
        metadata.tempo = Some(WavTempo {
            bpm: asset.source_bpm as f32,
            beats: asset.source_beats as u32,
            numerator: 4,
            denominator: 4,
            root_key: None,
            one_shot: false,
        });
    }
    metadata
        .validate(Some(u64::from(frames)))
        .map_err(|e| e.to_string())?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(write(&second_deck, &audio).is_err());
    }
}
