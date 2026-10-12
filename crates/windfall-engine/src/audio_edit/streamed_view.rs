//! Chunk reader using the editor/playback Region and fade laws. Prepared away
//! from State/audio; caller spools blocks and checks cancellation between them.
use crate::{SamplePool, clips::equal_power, voice::Region};
use windfall_core::{AudioBuffer, PanLaw};
use windfall_project::{AudioViewTiming, Clip, ClipContent, ClipStretch};
pub struct StreamedView {
    audio: AudioBuffer,
    region: Region,
    timing: AudioViewTiming,
    speed: f64,
    gain: f32,
    left: f32,
    right: f32,
}
impl StreamedView {
    pub fn new(pool: &SamplePool, clip: &Clip, timing: AudioViewTiming) -> Result<Self, String> {
        Self::new_with_checkpoint(pool, clip, timing, &mut |_| Ok(()))
    }
    pub fn new_with_checkpoint(
        pool: &SamplePool,
        clip: &Clip,
        timing: AudioViewTiming,
        checkpoint: &mut dyn FnMut(usize) -> Result<(), String>,
    ) -> Result<Self, String> {
        timing.check()?;
        let ClipContent::Audio {
            sample,
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
        let source = pool.get(sample).ok_or("Audio source unavailable.")?;
        if !matches!(source.channels(), 1 | 2) {
            return Err("Separation supports mono/stereo.".into());
        }
        // Spectral preparation belongs to the established bounded stretch cache.
        // Refuse large unprepared spectral sources instead of allocating a song.
        if matches!(stretch, ClipStretch::Spectral { .. })
            && source.duration_secs() > super::MAX_EDIT_SECONDS
        {
            return Err(
                "For long-song separation, render the spectral stretch to an audio asset first."
                    .into(),
            );
        }
        let audio = pool
            .clip_audio(sample, stretch, pitch)
            .ok_or("Audio source unavailable.")?;
        let speed = if matches!(stretch, ClipStretch::Tape) {
            2.0_f64.powf(f64::from(pitch) / 12.0)
        } else {
            1.0
        };
        let gain = if normalize {
            let mut peak = 0.0_f32;
            for block in audio.samples().chunks(4096) {
                checkpoint(block.len())?;
                for x in block {
                    if !x.is_finite() {
                        return Err("Nonfinite source audio.".into());
                    }
                    peak = peak.max(x.abs());
                }
            }
            if peak >= 1e-8 {
                (gain / peak).min(windfall_project::MAX_GAIN)
            } else {
                gain
            }
        } else {
            gain
        };
        let (left, right) = PanLaw::Balance.gains(pan);
        let region = Region {
            first: 0,
            frames: audio.frames(),
            reverse,
        };
        Ok(Self {
            audio,
            region,
            timing,
            speed,
            gain,
            left,
            right,
        })
    }
    pub fn read(&self, start: u64, output: &mut [f32]) -> Result<(), String> {
        if !output.len().is_multiple_of(2)
            || output.len() > 4096
            || start
                .checked_add((output.len() / 2) as u64)
                .is_none_or(|n| n > (self.timing.duration_frames - 1e-6).ceil() as u64)
        {
            return Err("Streamed clip read bounds.".into());
        }
        for (i, output) in output.chunks_exact_mut(2).enumerate() {
            let frame = start as f64 + i as f64;
            let mut gain = self.gain;
            if self.timing.fade_in_frames > 0.0 {
                gain *= equal_power(frame / self.timing.fade_in_frames);
            }
            if self.timing.fade_out_frames > 0.0 {
                gain *= equal_power(
                    (self.timing.duration_frames - frame) / self.timing.fade_out_frames,
                );
            }
            let position = self.timing.offset_frames * self.speed + frame * self.speed;
            let (left, right) = if position < self.audio.frames() as f64 {
                self.region.read(&self.audio, position)
            } else {
                (0.0, 0.0)
            };
            output[0] = left * gain * self.left;
            output[1] = right * gain * self.right;
        }
        if output.iter().any(|x| !x.is_finite()) {
            return Err("Nonfinite streamed audio.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_view_matches_editor_for_odd_chunks_reverse_pitch_pan_and_fades() {
        let source = AudioBuffer::from_interleaved(
            8000,
            2,
            (0..6000).map(|i| (i as f32 * 0.13).sin() * 0.4).collect(),
        );
        let mut pool = SamplePool::new();
        let sample = windfall_project::SampleId(1);
        pool.insert(sample, source);
        let clip = Clip {
            id: windfall_project::ClipId(2),
            track: windfall_project::PlaylistTrackId(3),
            start: 0,
            length: 960,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample,
                mixer_track: windfall_project::TrackId(0),
                output: Default::default(),
                normalize: true,
                gain: 0.5,
                pan: -0.3,
                reverse: true,
                pitch: 3.0,
                stretch: ClipStretch::Tape,
                fade_in: 0,
                fade_out: 0,
            },
        };
        let timing = AudioViewTiming {
            duration_frames: 2003.0,
            offset_frames: 11.5,
            fade_in_frames: 113.0,
            fade_out_frames: 231.0,
        };
        let reference = super::super::render_view_timed(&pool, &clip, &timing).unwrap();
        let stream = StreamedView::new(&pool, &clip, timing).unwrap();
        let mut output = vec![0.0; reference.samples().len()];
        for (index, block) in output.chunks_mut(126).enumerate() {
            stream.read((index * 63) as u64, block).unwrap();
        }
        assert_eq!(output, reference.samples());
    }
}
