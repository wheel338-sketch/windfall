//! Worker-only, bounded rendering of an already prepared source. The reader
//! may use loaded PCM or an anonymous file containing a spectral preparation.
use crate::{clips::equal_power, voice::hermite};
use windfall_core::PanLaw;
use windfall_project::{AudioViewTiming, Clip, ClipContent, ClipStretch};

/// Same view law as `render_view_timed`: offset, reverse, tape pitch, gain,
/// normalization, balance pan and tempo-aware fades. Emits at most 4096 samples.
/// Empty emissions checkpoint normalization without retaining its scan.
pub fn stream_prepared_view(
    clip: &Clip,
    timing: &AudioViewTiming,
    source_frames: usize,
    mut read: impl FnMut(usize) -> Result<(f32, f32), String>,
    mut emit: impl FnMut(&[f32]) -> Result<(), String>,
) -> Result<(), String> {
    timing.check()?;
    let ClipContent::Audio {
        gain,
        pan,
        normalize,
        reverse,
        pitch,
        stretch,
        ..
    } = clip.content
    else {
        return Err("Select an audio clip.".into());
    };
    let frames = (timing.duration_frames - 1e-6).ceil();
    if !frames.is_finite() || frames < 1.0 || frames > u32::MAX as f64 || source_frames == 0 {
        return Err("Streaming clip shape is invalid.".into());
    }
    let speed = if matches!(stretch, ClipStretch::Tape) {
        2.0_f64.powf(f64::from(pitch) / 12.0)
    } else {
        1.0
    };
    let skip = timing.offset_frames * speed;
    let (left, right) = PanLaw::Balance.gains(pan);
    let gain = if normalize {
        let mut peak = 0.0_f32;
        for frame in 0..source_frames {
            if frame % 2048 == 0 {
                emit(&[])?;
            }
            let (a, b) = read(frame)?;
            if !a.is_finite() || !b.is_finite() {
                return Err("Nonfinite prepared source.".into());
            }
            peak = peak.max(a.abs()).max(b.abs());
        }
        if peak >= 1e-8 {
            (gain / peak).min(windfall_project::MAX_GAIN)
        } else {
            gain
        }
    } else {
        gain
    };
    let mut block = [0.0_f32; 4096];
    for start in (0..frames as usize).step_by(2048) {
        emit(&[])?;
        let count = 2048.min(frames as usize - start);
        for offset in 0..count {
            let frame = start + offset;
            let position = skip + frame as f64 * speed;
            let mut at = |index: isize| {
                if index < 0 || index as usize >= source_frames {
                    return Ok((0.0, 0.0));
                }
                read(if reverse {
                    source_frames - 1 - index as usize
                } else {
                    index as usize
                })
            };
            let (a, b) = if position < source_frames as f64 {
                let index = position.floor() as isize;
                let fraction = (position - index as f64) as f32;
                let current = at(index)?;
                if fraction == 0.0 {
                    current
                } else {
                    let before = at(index - 1)?;
                    let next = at(index + 1)?;
                    let after = at(index + 2)?;
                    (
                        hermite(before.0, current.0, next.0, after.0, fraction),
                        hermite(before.1, current.1, next.1, after.1, fraction),
                    )
                }
            } else {
                (0.0, 0.0)
            };
            let mut level = gain;
            if timing.fade_in_frames > 0.0 {
                level *= equal_power(frame as f64 / timing.fade_in_frames);
            }
            if timing.fade_out_frames > 0.0 {
                level *=
                    equal_power((timing.duration_frames - frame as f64) / timing.fade_out_frames);
            }
            block[offset * 2] = a * level * left;
            block[offset * 2 + 1] = b * level * right;
        }
        if block[..count * 2].iter().any(|sample| !sample.is_finite()) {
            return Err("Nonfinite streamed view.".into());
        }
        emit(&block[..count * 2])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SamplePool;
    use windfall_core::AudioBuffer;
    use windfall_project::{ClipId, PlaylistTrackId, SampleId, TrackId};

    #[test]
    fn streamed_view_matches_timed_view_for_reverse_pitch_normalization_and_fades() {
        let audio = AudioBuffer::from_interleaved(
            8000,
            2,
            (0..24000)
                .flat_map(|frame| {
                    let sample =
                        (std::f32::consts::TAU * 220.0 * frame as f32 / 8000.0).sin() * 0.2;
                    [sample, sample * -0.5]
                })
                .collect(),
        );
        let mut pool = SamplePool::new();
        pool.insert(SampleId(6), audio.clone());
        for reverse in [false, true] {
            for pitch in [0.0, 2.75] {
                let clip = Clip {
                    id: ClipId(8),
                    track: PlaylistTrackId(7),
                    start: 0,
                    length: 1920,
                    offset: 0,
                    muted: false,
                    content: ClipContent::Audio {
                        sample: SampleId(6),
                        mixer_track: TrackId::MASTER,
                        output: Default::default(),
                        normalize: true,
                        gain: 0.8,
                        pan: 0.25,
                        fade_in: 0,
                        fade_out: 0,
                        reverse,
                        pitch,
                        stretch: ClipStretch::Tape,
                    },
                };
                let timing = AudioViewTiming {
                    duration_frames: 20000.25,
                    offset_frames: 150.5,
                    fade_in_frames: 240.0,
                    fade_out_frames: 480.0,
                };
                let expected = super::super::render_view_timed(&pool, &clip, &timing).unwrap();
                let mut actual = Vec::new();
                stream_prepared_view(
                    &clip,
                    &timing,
                    audio.frames(),
                    |frame| Ok(audio.stereo_frame(frame)),
                    |block| {
                        assert!(block.len() <= 4096);
                        actual.extend_from_slice(block);
                        Ok(())
                    },
                )
                .unwrap();
                assert_eq!(actual, expected.samples());
            }
        }
    }
}
