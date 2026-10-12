//! Stretching a whole buffer at once.

use windfall_core::AudioBuffer;

use crate::stretcher::{MAX_PITCH_SEMITONES, MAX_TIME_RATIO, MIN_TIME_RATIO, Quality, Stretcher};

/// Frames worked on at a time.
const CHUNK: usize = 4_096;

/// Whole-source spectral processing with bounded planar IO and no retained
/// output. Uses the same exact latency/clock as the ordinary offline renderer.
/// The caller owns publication and checkpoints via empty `emit` calls before
/// construction and between bounded blocks. False cancels without finishing.
pub fn stretch_blocks_cancellable(
    buffer: &AudioBuffer,
    time_ratio: f64,
    pitch_semitones: f64,
    quality: Quality,
    preserve_formants: bool,
    emit: &mut dyn FnMut(&[f32]) -> bool,
) -> bool {
    if !emit(&[]) || !matches!(buffer.channels(), 1 | 2) {
        return false;
    }
    let channels = usize::from(buffer.channels());
    let frames = buffer.frames();
    let wanted = stretched_frames(frames, time_ratio);
    if wanted == 0 {
        return false;
    }
    let pitch = if pitch_semitones.is_finite() {
        pitch_semitones.clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES)
    } else {
        0.0
    };
    if wanted == frames && pitch == 0.0 {
        for block in buffer.samples().chunks(CHUNK) {
            if !emit(&[]) || !emit(block) {
                return false;
            }
        }
        return emit(&[]);
    }
    let mut stretcher = Stretcher::with_quality(channels, buffer.sample_rate(), quality);
    stretcher.set_pitch_semitones(pitch);
    stretcher.set_formant_preservation(preserve_formants);
    let lead = stretcher.begin_exact(frames, wanted);
    // At most 2048 output frames / minimum ratio .25 = 8192 input frames.
    let mut planar = [vec![0.0_f32; 8193], vec![0.0_f32; 8193]];
    let mut output = [vec![0.0_f32; 2048], vec![0.0_f32; 2048]];
    let mut interleaved = [0.0_f32; CHUNK];
    let (mut position, mut produced) = (0_usize, 0_usize);
    while produced < lead + wanted {
        if !emit(&[]) {
            return false;
        }
        let count = 2048.min(lead + wanted - produced);
        let take = stretcher.input_frames_needed(count);
        if take > planar[0].len() {
            return false;
        }
        for frame in 0..take {
            for (channel, input) in planar.iter_mut().enumerate().take(channels) {
                input[frame] = if position + frame < frames {
                    buffer.samples()[(position + frame) * channels + channel]
                } else {
                    0.0
                };
            }
        }
        let input = [&planar[0][..take], &planar[1][..take]];
        let (left, right) = output.split_at_mut(1);
        let mut destination = [&mut left[0][..count], &mut right[0][..count]];
        position += stretcher.process(&input[..channels], &mut destination[..channels]);
        let skip = lead.saturating_sub(produced).min(count);
        for frame in skip..count {
            for (channel, output) in output.iter().enumerate().take(channels) {
                interleaved[(frame - skip) * channels + channel] = output[frame];
            }
        }
        if count > skip && !emit(&interleaved[..(count - skip) * channels]) {
            return false;
        }
        produced += count;
    }
    emit(&[])
}

/// Number of frames [`stretch`] returns for a buffer of `frames` frames:
/// `frames * time_ratio`, rounded, with the ratio forced into
/// [`MIN_TIME_RATIO`] to [`MAX_TIME_RATIO`].
pub fn stretched_frames(frames: usize, time_ratio: f64) -> usize {
    let ratio = if time_ratio.is_finite() {
        time_ratio
    } else {
        1.0
    };
    (frames as f64 * ratio.clamp(MIN_TIME_RATIO, MAX_TIME_RATIO)).round() as usize
}

/// Returns `buffer` made `time_ratio` times as long and transposed by
/// `pitch_semitones`, each without changing the other.
///
/// The result has exactly [`stretched_frames`] frames, at the sample rate
/// and with the channels of `buffer`. It starts where the audio starts and
/// ends where it ends: the stretcher's latency is taken off the front, and
/// the input is spread over the output's length exactly, so frame `n` of
/// the output is frame `n / time_ratio` of the input.
///
/// A time ratio of 1 with a pitch of 0 returns the audio as it is. The
/// call allocates and takes a while, so it belongs on a worker thread.
pub fn stretch(
    buffer: &AudioBuffer,
    time_ratio: f64,
    pitch_semitones: f64,
    quality: Quality,
) -> AudioBuffer {
    stretch_with_formants(buffer, time_ratio, pitch_semitones, quality, false)
}

/// Offline rendering with optional approximate voiced-spectrum formant preservation.
pub fn stretch_with_formants(
    buffer: &AudioBuffer,
    time_ratio: f64,
    pitch_semitones: f64,
    quality: Quality,
    preserve_formants: bool,
) -> AudioBuffer {
    stretch_with_formants_cancellable(
        buffer,
        time_ratio,
        pitch_semitones,
        quality,
        preserve_formants,
        &mut || true,
    )
    .expect("an uncancelled stretch always completes")
}

/// The same offline algorithm, with cancellation checked before construction and
/// every 4096 output frames. A cancelled result is discarded on the worker.
pub fn stretch_with_formants_cancellable(
    buffer: &AudioBuffer,
    time_ratio: f64,
    pitch_semitones: f64,
    quality: Quality,
    preserve_formants: bool,
    keep_going: &mut dyn FnMut() -> bool,
) -> Option<AudioBuffer> {
    if !keep_going() {
        return None;
    }
    let channels = usize::from(buffer.channels());
    let frames = buffer.frames();
    let wanted = stretched_frames(frames, time_ratio);
    let pitch = if pitch_semitones.is_finite() {
        pitch_semitones.clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES)
    } else {
        0.0
    };
    if wanted == 0 || (wanted == frames && pitch == 0.0) {
        let samples = buffer.samples()[..wanted * channels].to_vec();
        return Some(AudioBuffer::from_interleaved(
            buffer.sample_rate(),
            buffer.channels(),
            samples,
        ));
    }

    let planar: Vec<Vec<f32>> = (0..channels)
        .map(|channel| {
            let samples = buffer.samples().iter().skip(channel);
            samples.step_by(channels).copied().collect()
        })
        .collect();
    let mut stretcher = Stretcher::with_quality(channels, buffer.sample_rate(), quality);
    stretcher.set_pitch_semitones(pitch);
    stretcher.set_formant_preservation(preserve_formants);
    let lead = stretcher.begin_exact(frames, wanted);

    let mut scratch = vec![vec![0.0_f32; CHUNK]; channels];
    let mut data = Vec::with_capacity(wanted * channels);
    let (mut position, mut produced) = (0, 0);
    while produced < lead + wanted {
        if !keep_going() {
            return None;
        }
        let chunk = CHUNK.min(lead + wanted - produced);
        let input: Vec<&[f32]> = planar
            .iter()
            .map(|channel| &channel[position.min(frames)..])
            .collect();
        let mut output: Vec<&mut [f32]> = scratch
            .iter_mut()
            .map(|channel| &mut channel[..chunk])
            .collect();
        position += stretcher.process(&input, &mut output);
        let skip = lead.saturating_sub(produced).min(chunk);
        for frame in skip..chunk {
            data.extend(scratch.iter().map(|channel| channel[frame]));
        }
        produced += chunk;
    }
    Some(AudioBuffer::from_interleaved(
        buffer.sample_rate(),
        buffer.channels(),
        data,
    ))
}

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    fn bounded_spectral_stream_matches_exact_offline_clock_for_all_ratios() {
        let audio = AudioBuffer::from_interleaved(
            8000,
            2,
            (0..16000)
                .flat_map(|frame| {
                    let sample =
                        (std::f32::consts::TAU * 220.0 * frame as f32 / 8000.0).sin() * 0.2;
                    [sample, sample * -0.5]
                })
                .collect(),
        );
        for ratio in [0.25, 0.73, 1.0, 1.67, 4.0] {
            for pitch in [0.0, 3.0] {
                let expected = stretch_with_formants(&audio, ratio, pitch, Quality::High, false);
                let mut actual = Vec::new();
                assert!(stretch_blocks_cancellable(
                    &audio,
                    ratio,
                    pitch,
                    Quality::High,
                    false,
                    &mut |samples| {
                        assert!(samples.len() <= 4096);
                        actual.extend_from_slice(samples);
                        true
                    }
                ));
                assert_eq!(actual, expected.samples(), "ratio={ratio}, pitch={pitch}");
            }
        }
    }

    #[test]
    fn spectral_stream_cancellation_stops_before_emitting_another_block() {
        let audio = AudioBuffer::from_interleaved(8000, 1, vec![0.2; 16000]);
        let mut emitted = 0;
        assert!(!stretch_blocks_cancellable(
            &audio,
            2.0,
            3.0,
            Quality::High,
            false,
            &mut |samples| {
                if samples.is_empty() {
                    return true;
                }
                emitted += samples.len();
                false
            }
        ));
        assert!(emitted > 0 && emitted <= 4096);
    }
}
