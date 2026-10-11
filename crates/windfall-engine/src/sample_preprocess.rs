//! Offline treatments of a sampler channel's sample: trimming, reversing,
//! DC removal, polarity, channel swap, fades, a crossfaded loop seam and
//! normalizing.
//!
//! This is control-side work on a whole buffer. It never runs on the audio
//! thread: the session renders a new sample with it on a worker, writes that
//! to a file and attaches it to the channel in one undoable edit. The
//! sampler's reverse switch and playback loop crossfade stay what they are,
//! settings that change how an unchanged sample is read; this module is the
//! one that changes the audio.

use windfall_core::AudioBuffer;

/// The longest sample that can be treated, as stored 32-bit samples. The
/// audio editor has the same bound.
pub const MAX_PREPROCESS_BYTES: usize = crate::audio_edit::MAX_EDIT_BYTES;
/// The longest fade, in milliseconds.
pub const MAX_FADE_MS: f32 = 60_000.0;
/// What normalizing raises the loudest sample to.
pub const NORMALIZE_PEAK: f32 = 1.0;

/// The loop of a sampler, as fractions of the region that plays, in source
/// order: the sampler's own `loop_start` and `loop_end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopSeam {
    pub start: f32,
    pub end: f32,
    /// How much of the loop blends across the seam, 0 to 1. At most half
    /// the loop is used.
    pub amount: f32,
}

/// What to do to a sample. The steps run in the order of the fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplePreprocess {
    /// The region the sampler plays, as fractions of the sample.
    pub start: f32,
    pub end: f32,
    /// Keep only that region. Otherwise the whole sample is treated and
    /// the region stays a setting.
    pub trim: bool,
    /// The sampler plays the region backwards.
    pub reversed: bool,
    /// Turn the audio around so the sampler no longer has to.
    pub reverse: bool,
    pub remove_dc: bool,
    pub invert_polarity: bool,
    pub swap_stereo: bool,
    pub fade_in_ms: f32,
    pub fade_out_ms: f32,
    pub loop_crossfade: Option<LoopSeam>,
    pub normalize: bool,
}

impl Default for SamplePreprocess {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
            trim: false,
            reversed: false,
            reverse: false,
            remove_dc: false,
            invert_polarity: false,
            swap_stereo: false,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            loop_crossfade: None,
            normalize: false,
        }
    }
}

impl SamplePreprocess {
    /// Whether any step would change the audio.
    pub fn changes_audio(&self) -> bool {
        (self.trim && (self.start > 0.0 || self.end < 1.0))
            || self.reverse
            || self.remove_dc
            || self.invert_polarity
            || self.swap_stereo
            || self.fade_in_ms > 0.0
            || self.fade_out_ms > 0.0
            || self.loop_crossfade.is_some_and(|seam| seam.amount > 0.0)
            || self.normalize
    }
}

/// A treated sample and the sampler settings that go with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Preprocessed {
    pub audio: AudioBuffer,
    /// The region to play, as fractions of the new sample.
    pub start: f32,
    pub end: f32,
    /// Whether the sampler still has to play the region backwards.
    pub reversed: bool,
    /// The loop, as fractions of the new region in source order. `None`
    /// when no seam was asked for, which leaves the sampler's loop alone.
    pub loop_points: Option<(f32, f32)>,
    /// Frames blended at the loop seam.
    pub crossfade_frames: usize,
    /// The gain normalizing applied, 1 when it was not asked for.
    pub normalize_gain: f32,
}

fn fraction(value: f32, fallback: f32) -> f64 {
    if value.is_finite() {
        f64::from(value.clamp(0.0, 1.0))
    } else {
        f64::from(fallback)
    }
}

/// The frames the sampler plays: the plan compiler's rounding.
fn region(frames: usize, start: f32, end: f32) -> (usize, usize) {
    let first = ((fraction(start, 0.0) * frames as f64).round() as usize)
        .min(frames.saturating_sub(1));
    let last = ((fraction(end, 1.0) * frames as f64).round() as usize)
        .clamp((first + 1).min(frames), frames);
    (first, last)
}

fn reverse_frames(data: &mut [f32], channels: usize) {
    let frames = data.len() / channels;
    for frame in 0..frames / 2 {
        for channel in 0..channels {
            data.swap(
                frame * channels + channel,
                (frames - 1 - frame) * channels + channel,
            );
        }
    }
}

/// Applies the treatments to a copy of `source`.
///
/// The steps: keep the played region (`trim`); turn the audio around
/// (`reverse`); subtract each channel's mean (`remove_dc`); flip the sign
/// (`invert_polarity`); exchange left and right (`swap_stereo`); fade the
/// played region in and out with equal-power curves; blend the loop seam;
/// and last, with one gain for every channel, raise the peak to
/// [`NORMALIZE_PEAK`].
///
/// The loop seam is blended in the order the sampler plays the region. When
/// there is audio before the loop start, the end of the loop fades into the
/// audio that leads into the loop, and the loop keeps its length. When the
/// loop starts at the beginning of the region, the audio that follows a
/// shortened loop fades into the loop's start instead, and the returned
/// loop ends earlier by the length of the blend. Either way the frame after
/// the loop end is the one the original audio continued with.
pub fn preprocess(source: &AudioBuffer, options: &SamplePreprocess) -> Result<Preprocessed, String> {
    let channels = usize::from(source.channels());
    if channels == 0 || channels > 2 {
        return Err("Sample pre-processing supports mono and stereo samples.".into());
    }
    if source.frames() == 0 {
        return Err("The sample is empty.".into());
    }
    if std::mem::size_of_val(source.samples()) > MAX_PREPROCESS_BYTES {
        return Err("The sample is too long to pre-process (64 MiB of audio at most).".into());
    }
    if source.samples().iter().any(|sample| !sample.is_finite()) {
        return Err("The sample contains non-finite audio.".into());
    }
    for fade in [options.fade_in_ms, options.fade_out_ms] {
        if !fade.is_finite() || !(0.0..=MAX_FADE_MS).contains(&fade) {
            return Err("A fade must be between 0 and 60000 ms.".into());
        }
    }
    if let Some(seam) = options.loop_crossfade
        && (!seam.amount.is_finite() || !(0.0..=1.0).contains(&seam.amount))
    {
        return Err("The loop crossfade must be between 0 and 1.".into());
    }
    if options.swap_stereo && channels != 2 {
        return Err("Only a stereo sample has sides to swap.".into());
    }

    let (first, last) = region(source.frames(), options.start, options.end);
    // The buffer being built, and the region of it the sampler plays.
    let (mut data, mut play_first, mut play_last) = if options.trim {
        (
            source.samples()[first * channels..last * channels].to_vec(),
            0,
            last - first,
        )
    } else {
        (source.samples().to_vec(), first, last)
    };
    let frames = data.len() / channels;

    let mut reversed = options.reversed;
    if options.reverse {
        reverse_frames(&mut data, channels);
        (play_first, play_last) = (frames - play_last, frames - play_first);
        reversed = !reversed;
    }

    if options.remove_dc {
        for channel in 0..channels {
            let sum: f64 = data
                .iter()
                .skip(channel)
                .step_by(channels)
                .map(|sample| f64::from(*sample))
                .sum();
            let mean = (sum / frames as f64) as f32;
            for sample in data.iter_mut().skip(channel).step_by(channels) {
                *sample -= mean;
            }
        }
    }
    if options.invert_polarity {
        for sample in &mut data {
            *sample = -*sample;
        }
    }
    if options.swap_stereo {
        for frame in data.chunks_exact_mut(2) {
            frame.swap(0, 1);
        }
    }

    // From here on the work is in the order the sampler plays the region.
    let play_frames = play_last - play_first;
    let at = |index: usize| {
        if reversed {
            play_last - 1 - index
        } else {
            play_first + index
        }
    };
    let rate = f64::from(source.sample_rate());
    let fade_frames =
        |ms: f32| ((f64::from(ms) / 1000.0 * rate).round() as usize).min(play_frames);
    let fade_in = fade_frames(options.fade_in_ms);
    for index in 0..fade_in {
        let gain = crate::clips::equal_power(index as f64 / fade_in.max(2).saturating_sub(1) as f64);
        for channel in 0..channels {
            data[at(index) * channels + channel] *= gain;
        }
    }
    let fade_out = fade_frames(options.fade_out_ms);
    for index in 0..fade_out {
        let gain = crate::clips::equal_power(index as f64 / fade_out.max(2).saturating_sub(1) as f64);
        for channel in 0..channels {
            data[at(play_frames - 1 - index) * channels + channel] *= gain;
        }
    }

    let mut loop_points = None;
    let mut crossfade_frames = 0;
    if let Some(seam) = options.loop_crossfade {
        // Loop bounds in playing order, with the plan compiler's rounding.
        let source_first =
            ((fraction(seam.start, 0.0) * play_frames as f64).round() as usize).min(play_frames - 1);
        let source_end = ((fraction(seam.end, 1.0) * play_frames as f64).round() as usize)
            .clamp(source_first + 1, play_frames);
        let length = source_end - source_first;
        let loop_first = if reversed {
            play_frames - source_end
        } else {
            source_first
        };
        let mut loop_end = loop_first + length;
        let count = ((f64::from(seam.amount) * length as f64).round() as usize).min(length / 2);
        if count > 0 {
            // Equal-power weights, `(incoming, outgoing)`, `step` of `count`
            // steps through the blend.
            let weights = |step: usize| {
                (step as f32 / count as f32 * std::f32::consts::FRAC_PI_2).sin_cos()
            };
            if loop_first >= count {
                // The loop's tail takes on the audio that leads into it. Its
                // last frame becomes the frame before the loop start, so the
                // jump back to the start is the step the audio made there.
                for index in 0..count {
                    let (incoming, outgoing) = weights(index + 1);
                    for channel in 0..channels {
                        let lead = data[at(loop_first - count + index) * channels + channel];
                        let tail = &mut data[at(loop_end - count + index) * channels + channel];
                        *tail = *tail * outgoing + lead * incoming;
                    }
                }
            } else {
                // No audio before the loop: shorten the loop and let what
                // followed it fade into its start. The loop's first frame
                // becomes the frame that followed its new end.
                loop_end -= count;
                for index in 0..count {
                    let (incoming, outgoing) = weights(index);
                    for channel in 0..channels {
                        let follow = data[at(loop_end + index) * channels + channel];
                        let head = &mut data[at(loop_first + index) * channels + channel];
                        *head = *head * incoming + follow * outgoing;
                    }
                }
            }
            crossfade_frames = count;
        }
        // Back to fractions of the region in source order.
        let (first, end) = if reversed {
            (play_frames - loop_end, play_frames - loop_first)
        } else {
            (loop_first, loop_end)
        };
        loop_points = Some((
            (first as f64 / play_frames as f64) as f32,
            (end as f64 / play_frames as f64) as f32,
        ));
    }

    let mut normalize_gain = 1.0;
    if options.normalize {
        let peak = data.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        if peak == 0.0 {
            return Err("The sample is silent and cannot be normalized.".into());
        }
        let gain = f64::from(NORMALIZE_PEAK) / f64::from(peak);
        for sample in &mut data {
            *sample = (f64::from(*sample) * gain) as f32;
        }
        normalize_gain = gain as f32;
    }
    if data.iter().any(|sample| !sample.is_finite()) {
        return Err("Pre-processing produced non-finite audio.".into());
    }

    Ok(Preprocessed {
        audio: AudioBuffer::from_interleaved(source.sample_rate(), source.channels(), data),
        start: (play_first as f64 / frames as f64) as f32,
        end: (play_last as f64 / frames as f64) as f32,
        reversed,
        loop_points,
        crossfade_frames,
        normalize_gain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(data: Vec<f32>) -> AudioBuffer {
        AudioBuffer::from_interleaved(48_000, 1, data)
    }

    fn ramp(frames: usize) -> AudioBuffer {
        mono((0..frames).map(|frame| (frame + 1) as f32 / 1000.0).collect())
    }

    fn sine(frames: usize, period: f64) -> AudioBuffer {
        mono(
            (0..frames)
                .map(|frame| (frame as f64 / period * std::f64::consts::TAU).sin() as f32 * 0.5)
                .collect(),
        )
    }

    #[test]
    fn nothing_asked_for_returns_the_same_audio_and_settings() {
        let source = ramp(100);
        let options = SamplePreprocess {
            start: 0.25,
            end: 0.75,
            reversed: true,
            ..Default::default()
        };
        assert!(!options.changes_audio());
        let out = preprocess(&source, &options).unwrap();
        assert_eq!(out.audio.samples(), source.samples());
        assert_eq!((out.start, out.end, out.reversed), (0.25, 0.75, true));
        assert_eq!(out.loop_points, None);
        assert_eq!(out.normalize_gain, 1.0);
    }

    #[test]
    fn trim_keeps_the_played_frames_and_reverse_turns_them_around() {
        let source = ramp(8);
        let trimmed = preprocess(
            &source,
            &SamplePreprocess {
                start: 0.25,
                end: 0.75,
                trim: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(trimmed.audio.samples(), &source.samples()[2..6]);
        assert_eq!((trimmed.start, trimmed.end), (0.0, 1.0));

        // Reversing the audio clears the sampler's own reverse switch, and
        // the region follows its frames to the other end of the buffer.
        let turned = preprocess(
            &source,
            &SamplePreprocess {
                start: 0.25,
                end: 0.5,
                reversed: true,
                reverse: true,
                ..Default::default()
            },
        )
        .unwrap();
        let mut expected = source.samples().to_vec();
        expected.reverse();
        assert_eq!(turned.audio.samples(), expected);
        assert!(!turned.reversed);
        assert_eq!((turned.start, turned.end), (0.5, 0.75));
        // The frames it plays, in the order it plays them, are unchanged.
        let before: Vec<f32> = source.samples()[2..4].iter().rev().copied().collect();
        assert_eq!(&turned.audio.samples()[4..6], before);
    }

    #[test]
    fn stereo_frames_stay_paired_through_reverse_swap_and_polarity() {
        let source = AudioBuffer::from_interleaved(
            48_000,
            2,
            vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3],
        );
        let out = preprocess(
            &source,
            &SamplePreprocess {
                reverse: true,
                swap_stereo: true,
                invert_polarity: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.audio.samples(), [0.3, -0.3, 0.2, -0.2, 0.1, -0.1]);
        assert!(
            preprocess(
                &ramp(4),
                &SamplePreprocess {
                    swap_stereo: true,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn dc_is_removed_per_channel_and_normalize_uses_one_gain() {
        let source = AudioBuffer::from_interleaved(
            48_000,
            2,
            vec![0.5, 0.1, 0.3, -0.1, 0.5, 0.1, 0.3, -0.1],
        );
        let out = preprocess(
            &source,
            &SamplePreprocess {
                remove_dc: true,
                normalize: true,
                ..Default::default()
            },
        )
        .unwrap();
        let samples = out.audio.samples();
        // Left was 0.4 +/- 0.1 and right 0 +/- 0.1: both become +/- 0.1,
        // and one gain of ten brings the peak to full scale.
        for (index, expected) in [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0].iter().enumerate() {
            assert!((samples[index] - expected).abs() < 1e-5, "{samples:?}");
        }
        assert!((out.normalize_gain - 10.0).abs() < 1e-3);
        let silent = preprocess(
            &mono(vec![0.0; 16]),
            &SamplePreprocess {
                normalize: true,
                ..Default::default()
            },
        );
        assert!(silent.unwrap_err().contains("silent"));
    }

    #[test]
    fn fades_shape_the_played_region_in_playing_order() {
        let source = mono(vec![1.0; 480]);
        let out = preprocess(
            &source,
            &SamplePreprocess {
                fade_in_ms: 1.0,
                fade_out_ms: 2.0,
                ..Default::default()
            },
        )
        .unwrap();
        let samples = out.audio.samples();
        assert_eq!(samples[0], 0.0);
        assert!(samples[..48].windows(2).all(|pair| pair[0] < pair[1]));
        assert!((samples[47] - 1.0).abs() < 1e-6);
        assert_eq!(samples[100], 1.0);
        assert_eq!(samples[479], 0.0);
        assert!(samples[384..].windows(2).all(|pair| pair[0] > pair[1]));

        // Played backwards, the fade-in is at the end of the stored audio.
        let backwards = preprocess(
            &source,
            &SamplePreprocess {
                reversed: true,
                fade_in_ms: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(backwards.audio.samples()[479], 0.0);
        assert_eq!(backwards.audio.samples()[0], 1.0);
    }

    /// The jump a forward loop makes from its last frame back to its first.
    fn seam_step(audio: &AudioBuffer, start: f32, end: f32) -> f32 {
        let frames = audio.frames();
        let first = (f64::from(start) * frames as f64).round() as usize;
        let last = (f64::from(end) * frames as f64).round() as usize;
        (audio.samples()[first] - audio.samples()[last - 1]).abs()
    }

    fn largest_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_loop_with_audio_before_it_keeps_its_length_and_loses_its_click() {
        // A 100-frame sine looped over 475 frames: the loop ends a quarter
        // of a cycle short, at the bottom of the wave, and jumps to zero.
        let source = sine(2000, 100.0);
        let seam = LoopSeam {
            start: 0.25,
            end: 0.4875,
            amount: 0.2,
        };
        let step = largest_step(source.samples());
        assert!(seam_step(&source, seam.start, seam.end) > step * 1.9);
        let out = preprocess(
            &source,
            &SamplePreprocess {
                loop_crossfade: Some(seam),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.loop_points, Some((0.25, 0.4875)));
        assert_eq!(out.crossfade_frames, 95);
        // The seam is now no rougher than the sine itself.
        assert!(seam_step(&out.audio, 0.25, 0.4875) <= step * 1.001);
        assert!(largest_step(&out.audio.samples()[500..975]) <= step * 1.6);
        // Only the 95 frames before the loop end changed.
        assert_eq!(out.audio.samples()[..880], source.samples()[..880]);
        assert_eq!(out.audio.samples()[975..], source.samples()[975..]);
        assert_ne!(out.audio.samples()[880..975], source.samples()[880..975]);
    }

    #[test]
    fn a_loop_from_the_first_frame_is_shortened_to_make_its_blend() {
        let source = sine(1000, 100.0);
        let seam = LoopSeam {
            start: 0.0,
            end: 0.475,
            amount: 0.2,
        };
        let step = largest_step(source.samples());
        let out = preprocess(
            &source,
            &SamplePreprocess {
                loop_crossfade: Some(seam),
                ..Default::default()
            },
        )
        .unwrap();
        // 475 frames less the 95 that were blended into the start.
        assert_eq!(out.loop_points, Some((0.0, 0.38)));
        assert!(seam_step(&source, 0.0, 0.475) > step * 1.9);
        assert!(seam_step(&out.audio, 0.0, 0.38) <= step * 1.001);
        assert!(largest_step(&out.audio.samples()[..380]) <= step * 1.6);
        assert_eq!(out.audio.samples()[95..], source.samples()[95..]);
        assert_ne!(out.audio.samples()[..95], source.samples()[..95]);
    }

    #[test]
    fn a_reversed_region_blends_its_seam_in_the_order_it_plays() {
        // Played backwards the loop is read from frame 974 down to 500, and
        // the audio that leads into it is the frames above 974.
        let source = sine(2000, 100.0);
        let out = preprocess(
            &source,
            &SamplePreprocess {
                reversed: true,
                loop_crossfade: Some(LoopSeam {
                    start: 0.25,
                    end: 0.4875,
                    amount: 0.2,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.loop_points, Some((0.25, 0.4875)));
        assert!(out.reversed);
        let samples = out.audio.samples();
        let step = largest_step(source.samples());
        // Backwards, the loop's last frame is 500 and its first is 974.
        assert!((source.samples()[974] - source.samples()[500]).abs() > step * 1.9);
        assert!((samples[974] - samples[500]).abs() <= step * 1.001);
        assert_eq!(samples[..500], source.samples()[..500]);
        assert_eq!(samples[595..], source.samples()[595..]);
        assert_ne!(samples[500..595], source.samples()[500..595]);
    }

    #[test]
    fn unusable_input_is_refused() {
        let options = SamplePreprocess {
            normalize: true,
            ..Default::default()
        };
        assert!(preprocess(&mono(Vec::new()), &options).is_err());
        assert!(preprocess(&mono(vec![0.1, f32::NAN]), &options).is_err());
        let surround = AudioBuffer::from_interleaved(48_000, 3, vec![0.1; 9]);
        assert!(preprocess(&surround, &options).is_err());
        for bad in [
            SamplePreprocess {
                fade_in_ms: f32::NAN,
                ..Default::default()
            },
            SamplePreprocess {
                fade_out_ms: 1.0e9,
                ..Default::default()
            },
            SamplePreprocess {
                loop_crossfade: Some(LoopSeam {
                    start: 0.0,
                    end: 1.0,
                    amount: 2.0,
                }),
                ..Default::default()
            },
        ] {
            assert!(preprocess(&ramp(64), &bad).is_err());
        }
    }
}
