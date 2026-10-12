//! Bounded, immutable clip editing on workers. Uses playback's region reader,
//! spectral preparation and equal-power fade law; excludes mixer processing
//! and transport's transient de-click ramps.
use windfall_core::{AudioBuffer, PanLaw, samples_per_tick};
use windfall_ipc::{AudioEditOperation, AudioFadeDirection, AudioFadeShape, AudioNormalizeMode};
use windfall_project::{Clip, ClipContent, ClipStretch};

use crate::{SamplePool, clips::equal_power, voice::Region};

pub mod advanced;
pub mod clean;
pub mod process;
mod timeline;
pub use timeline::{render_view_timed, view_timing};

pub const MAX_EDIT_SECONDS: f64 = 120.0;
pub const MAX_EDIT_BYTES: usize = 64 * 1024 * 1024;

fn bounded(frames: f64, rate: u32, channels: u16) -> Result<usize, String> {
    if !frames.is_finite()
        || frames < 1.0
        || frames / f64::from(rate) > MAX_EDIT_SECONDS
        || frames * f64::from(channels) * 4.0 > MAX_EDIT_BYTES as f64
    {
        return Err("The audio editor supports up to two minutes and 64 MiB per buffer.".into());
    }
    Ok(frames as usize)
}

/// Renders the clip's visible window at the source rate, with silence after
/// the source ends, at a fixed tempo. Timeline editors use view_timing instead.
pub fn render_view(pool: &SamplePool, clip: &Clip, tempo: f64) -> Result<AudioBuffer, String> {
    let ClipContent::Audio {
        sample,
        fade_in,
        fade_out,
        ..
    } = clip.content
    else {
        return Err("Select an audio clip.".into());
    };
    if !tempo.is_finite() || tempo <= 0.0 {
        return Err("The project tempo is invalid.".into());
    }
    let source = pool.get(sample).ok_or("The source is unavailable.")?;
    let per_tick = samples_per_tick(tempo, f64::from(source.sample_rate()));
    render_view_timed(
        pool,
        clip,
        &windfall_project::AudioViewTiming {
            duration_frames: f64::from(clip.length) * per_tick,
            offset_frames: f64::from(clip.offset) * per_tick,
            fade_in_frames: f64::from(fade_in.min(clip.length)) * per_tick,
            fade_out_frames: f64::from(fade_out.min(clip.length)) * per_tick,
        },
    )
}

pub fn render_view_with_law(
    pool: &SamplePool,
    clip: &Clip,
    tempo: f64,
    law: PanLaw,
) -> Result<AudioBuffer, String> {
    let ClipContent::Audio {
        sample,
        fade_in,
        fade_out,
        ..
    } = clip.content
    else {
        return Err("Select an audio clip.".into());
    };
    if !tempo.is_finite() || tempo <= 0.0 {
        return Err("The project tempo is invalid.".into());
    }
    let rate = f64::from(
        pool.get(sample)
            .ok_or("The source is unavailable.")?
            .sample_rate(),
    );
    let per_tick = samples_per_tick(tempo, rate);
    render_view_timed_with_law(
        pool,
        clip,
        &windfall_project::AudioViewTiming {
            duration_frames: f64::from(clip.length) * per_tick,
            offset_frames: f64::from(clip.offset) * per_tick,
            fade_in_frames: f64::from(fade_in.min(clip.length)) * per_tick,
            fade_out_frames: f64::from(fade_out.min(clip.length)) * per_tick,
        },
        law,
    )
}

fn render_timed(
    pool: &SamplePool,
    clip: &Clip,
    timing: &windfall_project::AudioViewTiming,
) -> Result<AudioBuffer, String> {
    render_view_timed_with_law(pool, clip, timing, PanLaw::Balance)
}
pub fn render_view_timed_with_law(
    pool: &SamplePool,
    clip: &Clip,
    timing: &windfall_project::AudioViewTiming,
    law: PanLaw,
) -> Result<AudioBuffer, String> {
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
    let source = pool
        .get(sample)
        .ok_or("The clip's audio is not loaded. Reload samples first.")?;
    if !matches!(source.channels(), 1 | 2) {
        return Err("The audio editor supports mono and stereo sources.".into());
    }
    bounded(
        source.frames() as f64,
        source.sample_rate(),
        source.channels(),
    )?;
    if source.samples().iter().any(|s| !s.is_finite()) {
        return Err("The source contains non-finite audio samples.".into());
    }
    if let ClipStretch::Spectral { ratio, .. } = stretch {
        bounded(
            (source.frames() as f64 * ratio).round(),
            source.sample_rate(),
            source.channels(),
        )?;
    }
    let frames = bounded(
        (timing.duration_frames - 1e-6).ceil(),
        source.sample_rate(),
        2,
    )?;
    let audio = pool
        .clip_audio(sample, stretch, pitch)
        .ok_or("The source is unavailable.")?;
    let speed = if matches!(stretch, ClipStretch::Tape) {
        2.0_f64.powf(f64::from(pitch) / 12.0)
    } else {
        1.0
    };
    let skip = timing.offset_frames * speed;
    let region = Region {
        first: 0,
        frames: audio.frames(),
        reverse,
    };
    let (left, right) = law.gains(pan);
    let gain = if normalize {
        let peak = audio
            .samples()
            .iter()
            .fold(0.0_f32, |peak, s| peak.max(s.abs()));
        if peak >= 1e-8 {
            (gain / peak).min(windfall_project::MAX_GAIN)
        } else {
            gain
        }
    } else {
        gain
    };
    let mut data = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let mut level = gain;
        if timing.fade_in_frames > 0.0 {
            level *= equal_power(frame as f64 / timing.fade_in_frames);
        }
        if timing.fade_out_frames > 0.0 {
            level *= equal_power((timing.duration_frames - frame as f64) / timing.fade_out_frames);
        }
        let position = skip + frame as f64 * speed;
        let (a, b) = if position < audio.frames() as f64 {
            region.read(&audio, position)
        } else {
            (0.0, 0.0)
        };
        data.extend([a * level * left, b * level * right]);
    }
    if data.iter().any(|s| !s.is_finite()) {
        return Err("Rendering produced non-finite audio.".into());
    }
    Ok(AudioBuffer::from_interleaved(source.sample_rate(), 2, data))
}

/// The edited audio, and how far its first frame sits after the view's.
#[derive(Debug, Clone)]
pub struct Edited {
    pub audio: AudioBuffer,
    /// Frames removed from the front that the clip's start should follow:
    /// the selection start of a trim, or trimmed leading silence.
    pub lead: usize,
}

/// Selection is [start, end). All operations preserve channel interleaving.
/// Normalization uses one linked gain for all channels, targeting -1 dBFS.
pub fn edit(
    view: &AudioBuffer,
    operation: AudioEditOperation,
    start: u32,
    end: u32,
) -> Result<AudioBuffer, String> {
    apply(view, operation, start, end).map(|edited| edited.audio)
}

/// Every operation leaves samples outside the selection bit-identical;
/// length-changing ones move, but never alter, the frames after it.
pub fn apply(
    view: &AudioBuffer,
    operation: AudioEditOperation,
    start: u32,
    end: u32,
) -> Result<Edited, String> {
    let (start, end) = (start as usize, end as usize);
    if start >= end || end > view.frames() {
        return Err("Select at least one frame within the clip.".into());
    }
    bounded(view.frames() as f64, view.sample_rate(), view.channels())?;
    if view.samples().iter().any(|s| !s.is_finite()) {
        return Err("The audio contains non-finite samples.".into());
    }
    let channels = usize::from(view.channels());
    let rate = view.sample_rate();
    let first = start * channels;
    let last = end * channels;
    let samples = view.samples();
    let grown = |frames: usize| bounded(frames as f64, rate, view.channels());
    let spliced = |from: usize, middle: &[f32], to: usize| {
        let mut out = Vec::with_capacity(from + middle.len() + samples.len() - to);
        out.extend_from_slice(&samples[..from]);
        out.extend_from_slice(middle);
        out.extend_from_slice(&samples[to..]);
        out
    };
    let mut lead = 0;
    let data = match operation {
        AudioEditOperation::Trim | AudioEditOperation::Extract => {
            if operation == AudioEditOperation::Trim {
                lead = start;
            }
            samples[first..last].to_vec()
        }
        AudioEditOperation::Cut => {
            if start == 0 && end == view.frames() {
                return Err("Cut would leave an empty clip. Select a smaller range.".into());
            }
            spliced(first, &[], last)
        }
        AudioEditOperation::TrimSilence { threshold_db } => {
            let Some((keep, stop)) =
                clean::silence_bounds(samples, channels, start, end, threshold_db)?
            else {
                return Err(
                    "The whole selection is below the threshold. Use Cut or Silence instead."
                        .into(),
                );
            };
            if keep == start && stop == end {
                return Err("The selection has no leading or trailing silence to trim.".into());
            }
            if start == 0 {
                lead = keep;
            }
            spliced(first, &samples[keep * channels..stop * channels], last)
        }
        AudioEditOperation::InsertSilence { frames } => {
            if frames == 0 {
                return Err("Insert at least one frame of silence.".into());
            }
            grown(view.frames() + frames as usize)?;
            spliced(first, &vec![0.0; frames as usize * channels], first)
        }
        AudioEditOperation::Convolve { impulse, wet } => {
            let a = impulse.start_frame as usize;
            let b = impulse.end_frame as usize;
            if a >= b || b > view.frames() || b - a > 32768 {
                return Err(
                    "Capture an impulse range of 1 to 32768 frames within this clip.".into(),
                );
            }
            let convolved = advanced::convolve(
                &samples[first..last],
                &samples[a * channels..b * channels],
                channels,
                wet,
            )?;
            spliced(first, &convolved, last)
        }
        AudioEditOperation::StripSilence {
            threshold_db,
            min_silence_frames,
        } => {
            advanced::sound_regions(view, start, end, threshold_db, min_silence_frames)?;
            samples.to_vec() // Session attaches separate clips at these source positions.
        }
        AudioEditOperation::Script => {
            return Err("Run scripts through the sandboxed selection processor.".into());
        }
        AudioEditOperation::Stretch {
            ratio,
            semitones,
            quality,
            formants,
        } => {
            process::check_stretch(ratio, semitones)?;
            grown(view.frames() - (end - start) + process::stretched_frames(end - start, ratio))?;
            let stretched = process::stretch(
                &samples[first..last],
                rate,
                view.channels(),
                ratio,
                semitones,
                quality,
                formants,
            )?;
            spliced(first, &stretched, last)
        }
        AudioEditOperation::RemoveDc
        | AudioEditOperation::Declick { .. }
        | AudioEditOperation::ReduceNoise { .. } => {
            let mut out = samples.to_vec();
            match operation {
                AudioEditOperation::RemoveDc => clean::remove_dc(&mut out, channels, start, end),
                AudioEditOperation::Declick { strength } => {
                    if clean::declick(&mut out, channels, rate, start, end, strength)? == 0 {
                        return Err(
                            "No clicks were found in the selection. Raise the strength to detect smaller ones."
                                .into(),
                        );
                    }
                }
                AudioEditOperation::ReduceNoise {
                    reduction_db,
                    profile,
                } => clean::reduce_noise(
                    &mut out,
                    channels,
                    rate,
                    start,
                    end,
                    reduction_db,
                    profile.map(|range| (range.start_frame as usize, range.end_frame as usize)),
                )?,
                _ => unreachable!(),
            }
            out
        }
        _ => {
            let mut out = samples.to_vec();
            let selected = &mut out[first..last];
            match operation {
                AudioEditOperation::Normalize => {
                    process::normalize(selected, -1.0, AudioNormalizeMode::Peak)?
                }
                AudioEditOperation::NormalizeTo { target_db, mode } => {
                    process::normalize(selected, target_db, mode)?
                }
                AudioEditOperation::Gain { db } => process::gain(selected, db)?,
                AudioEditOperation::Equalize {
                    low_db,
                    mid_db,
                    high_db,
                } => advanced::equalize(selected, channels, rate, [low_db, mid_db, high_db])?,
                AudioEditOperation::Blur { amount } => advanced::blur(selected, channels, amount)?,
                AudioEditOperation::RepairClipping { threshold } => {
                    advanced::repair_clipping(selected, channels, threshold)?
                }
                AudioEditOperation::LoopTune { crossfade_frames } => {
                    advanced::loop_tune(selected, channels, crossfade_frames)?
                }
                AudioEditOperation::Reverse => {
                    for frame in 0..(end - start) / 2 {
                        for channel in 0..channels {
                            selected.swap(
                                frame * channels + channel,
                                (end - start - 1 - frame) * channels + channel,
                            );
                        }
                    }
                }
                AudioEditOperation::Silence => selected.fill(0.0),
                AudioEditOperation::FadeIn => {
                    process::fade(selected, channels, false, AudioFadeShape::EqualPower)
                }
                AudioEditOperation::FadeOut => {
                    process::fade(selected, channels, true, AudioFadeShape::EqualPower)
                }
                AudioEditOperation::Fade { direction, shape } => process::fade(
                    selected,
                    channels,
                    direction == AudioFadeDirection::Out,
                    shape,
                ),
                AudioEditOperation::SwapChannels => process::swap_channels(selected, channels)?,
                AudioEditOperation::MonoSum => process::mono_sum(selected, channels)?,
                AudioEditOperation::InvertPhase { channels: which } => {
                    process::invert_phase(selected, channels, which)?
                }
                _ => unreachable!(),
            }
            out
        }
    };
    if data.iter().any(|s| !s.is_finite()) {
        return Err("The edit produced non-finite audio.".into());
    }
    Ok(Edited {
        audio: AudioBuffer::from_interleaved(rate, view.channels(), data),
        lead,
    })
}

/// A sandbox returns only selected audio. Validate it, then splice through the
/// same immutable/bounded asset contract as built-in operations.
pub fn replace_selection(
    view: &AudioBuffer,
    start: u32,
    end: u32,
    replacement: AudioBuffer,
) -> Result<Edited, String> {
    if start >= end
        || end as usize > view.frames()
        || replacement.sample_rate() != view.sample_rate()
        || replacement.channels() != view.channels()
        || replacement.samples().iter().any(|s| !s.is_finite())
    {
        return Err("The script returned invalid selection audio.".into());
    }
    bounded(
        (view.frames() - (end - start) as usize + replacement.frames()) as f64,
        view.sample_rate(),
        view.channels(),
    )?;
    let channels = usize::from(view.channels());
    let mut samples = view.samples()[..start as usize * channels].to_vec();
    samples.extend_from_slice(replacement.samples());
    samples.extend_from_slice(&view.samples()[end as usize * channels..]);
    Ok(Edited {
        audio: AudioBuffer::from_interleaved(view.sample_rate(), view.channels(), samples),
        lead: 0,
    })
}

/// Envelope across channels, rather than their average: anti-phase stereo
/// must remain visible. Bucket count is caller bounded.
pub fn overview(audio: &AudioBuffer, buckets: usize) -> Vec<f32> {
    let buckets = buckets.min(1024);
    let mut peaks = Vec::with_capacity(buckets * 2);
    let channels = usize::from(audio.channels());
    for bucket in 0..buckets {
        let first = bucket * audio.frames() / buckets;
        let end = ((bucket + 1) * audio.frames() / buckets)
            .max(first + 1)
            .min(audio.frames());
        let span = &audio.samples()[first * channels..end * channels];
        peaks.extend([
            span.iter().copied().fold(0.0_f32, f32::min),
            span.iter().copied().fold(0.0_f32, f32::max),
        ]);
    }
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::{ClipId, ClipStretchQuality, PlaylistTrackId, SampleId, TrackId};
    fn source() -> AudioBuffer {
        AudioBuffer::from_interleaved(960, 2, vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3, 0.4, -0.4])
    }
    pub(super) fn clip() -> Clip {
        Clip {
            id: ClipId(1),
            track: PlaylistTrackId(2),
            start: 0,
            length: 4,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample: SampleId(3),
                mixer_track: TrackId::MASTER,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,
                stretch: ClipStretch::Tape,
            },
        }
    }
    #[test]
    fn exact_stereo_edits_leave_the_source_unchanged() {
        let view = source();
        let original = view.samples().to_vec();
        for operation in [AudioEditOperation::Trim, AudioEditOperation::Extract] {
            assert_eq!(
                edit(&view, operation, 1, 3).unwrap().samples(),
                &[0.2, -0.2, 0.3, -0.3]
            );
        }
        assert_eq!(
            edit(&view, AudioEditOperation::Reverse, 1, 3)
                .unwrap()
                .samples(),
            &[0.1, -0.1, 0.3, -0.3, 0.2, -0.2, 0.4, -0.4]
        );
        assert_eq!(
            edit(&view, AudioEditOperation::Cut, 1, 3)
                .unwrap()
                .samples(),
            &[0.1, -0.1, 0.4, -0.4]
        );
        assert_eq!(
            edit(&view, AudioEditOperation::Silence, 1, 3)
                .unwrap()
                .samples(),
            &[0.1, -0.1, 0.0, 0.0, 0.0, 0.0, 0.4, -0.4]
        );
        assert_eq!(
            edit(&view, AudioEditOperation::FadeIn, 1, 3)
                .unwrap()
                .samples(),
            &[0.1, -0.1, 0.0, 0.0, 0.3, -0.3, 0.4, -0.4]
        );
        assert_eq!(
            edit(&view, AudioEditOperation::FadeOut, 1, 3)
                .unwrap()
                .samples(),
            &[0.1, -0.1, 0.2, -0.2, 0.0, 0.0, 0.4, -0.4]
        );
        let normalized = edit(&view, AudioEditOperation::Normalize, 1, 3).unwrap();
        assert_eq!(&normalized.samples()[..2], &original[..2]);
        assert!((normalized.samples()[4] - windfall_core::db_to_gain(-1.0)).abs() < 1e-6);
        assert_eq!(normalized.samples()[4], -normalized.samples()[5]);
        assert_eq!(view.samples(), original);
        assert_eq!(
            overview(&view, 4),
            vec![-0.1, 0.1, -0.2, 0.2, -0.3, 0.3, -0.4, 0.4]
        );
    }
    #[test]
    fn view_uses_playback_offset_reverse_pitch_gain_pan_and_fades() {
        let mut pool = SamplePool::new();
        pool.insert(SampleId(3), source());
        let mut clip = clip(); // at 60 BPM / 960Hz: one frame per tick
        assert_eq!(
            render_view(&pool, &clip, 60.0).unwrap().samples(),
            source().samples()
        );
        clip.offset = 1;
        clip.length = 2;
        let ClipContent::Audio {
            reverse,
            gain,
            pan,
            fade_in,
            ..
        } = &mut clip.content
        else {
            unreachable!()
        };
        *reverse = true;
        *gain = 0.5;
        *pan = 1.0;
        *fade_in = 1;
        assert_eq!(
            render_view(&pool, &clip, 60.0).unwrap().samples(),
            &[0.0, -0.0, 0.0, -0.1]
        );
        clip = super::tests::clip();
        let ClipContent::Audio { pitch, .. } = &mut clip.content else {
            unreachable!()
        };
        *pitch = 12.0;
        assert_eq!(
            render_view(&pool, &clip, 60.0).unwrap().samples(),
            &[0.1, -0.1, 0.3, -0.3, 0.0, 0.0, 0.0, 0.0]
        );
    }
    #[test]
    fn spectral_view_has_independent_duration_and_preserves_source() {
        let mut pool = SamplePool::new();
        let audio = AudioBuffer::from_interleaved(
            48000,
            1,
            (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.2).collect(),
        );
        pool.insert(SampleId(3), audio.clone());
        let mut clip = clip();
        clip.length = 288;
        let ClipContent::Audio { stretch, pitch, .. } = &mut clip.content else {
            unreachable!()
        };
        *stretch = ClipStretch::Spectral {
            ratio: 1.5,
            quality: ClipStretchQuality::Fast,
            formants: false,
        };
        *pitch = 7.0;
        let view = render_view(&pool, &clip, 120.0).unwrap();
        assert_eq!(view.frames(), 7200);
        assert!(view.samples().iter().any(|s| s.abs() > 0.01));
        assert_eq!(pool.get(SampleId(3)).unwrap().samples(), audio.samples());
    }
    #[test]
    fn invalid_empty_nonfinite_and_oversized_edits_are_refused() {
        let view = source();
        for (start, end) in [(0, 0), (2, 1), (0, 5)] {
            assert!(edit(&view, AudioEditOperation::Trim, start, end).is_err());
        }
        assert!(edit(&view, AudioEditOperation::Cut, 0, 4).is_err());
        let silent = AudioBuffer::from_interleaved(960, 1, vec![0.0; 4]);
        assert!(edit(&silent, AudioEditOperation::Normalize, 0, 4).is_err());
        let bad = AudioBuffer::from_interleaved(960, 1, vec![f32::NAN]);
        assert!(edit(&bad, AudioEditOperation::Silence, 0, 1).is_err());
        assert!(bounded(120.0 * 48000.0 + 1.0, 48000, 2).is_err());
        assert!(bounded(120.0 * 192000.0, 192000, 2).is_err());
        let mut pool = SamplePool::new();
        pool.insert(SampleId(3), view);
        let mut clip = clip();
        clip.length = 960 * 121;
        assert!(render_view(&pool, &clip, 60.0).is_err());
    }

    use windfall_ipc::{AudioEditChannels, AudioEditRange};

    /// One second at 48 kHz: silence, a noisy tone, silence.
    fn take() -> AudioBuffer {
        let mut state = 9_u64;
        let mut data = Vec::with_capacity(96_000);
        for i in 0..48_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let noise = ((state >> 40) as f32 / (1u64 << 23) as f32 - 1.0) * 0.01;
            let tone = if (4800..43_200).contains(&i) {
                (i as f32 * 0.05).sin() * 0.4
            } else {
                0.0
            };
            let quiet = if (4800..43_200).contains(&i) {
                noise
            } else {
                noise * 0.01
            };
            data.extend([tone + quiet, (tone + quiet) * 0.5]);
        }
        AudioBuffer::from_interleaved(48_000, 2, data)
    }

    #[test]
    fn audio_edit_trim_and_insert_silence_have_exact_boundaries_and_lengths() {
        let view = take();
        let samples = view.samples();
        let trim = AudioEditOperation::TrimSilence {
            threshold_db: -60.0,
        };
        // Whole clip: both ends go, and the clip start follows the lead.
        let whole = apply(&view, trim, 0, 48_000).unwrap();
        assert_eq!(whole.lead, 4800);
        assert_eq!(whole.audio.frames(), 38_400);
        assert_eq!(whole.audio.samples(), &samples[9600..86_400]);
        // Inner selection: audio before and after it is kept in place.
        let inner = apply(&view, trim, 2400, 45_600).unwrap();
        assert_eq!(inner.lead, 0);
        assert_eq!(inner.audio.frames(), 2400 + 38_400 + 2400);
        assert_eq!(&inner.audio.samples()[..4800], &samples[..4800]);
        assert_eq!(&inner.audio.samples()[4800..81_600], &samples[9600..86_400]);
        assert_eq!(&inner.audio.samples()[81_600..], &samples[91_200..]);
        assert!(apply(&view, trim, 0, 4800).unwrap_err().contains("below"));
        assert!(
            apply(&view, trim, 4800, 43_200)
                .unwrap_err()
                .contains("no leading")
        );

        let insert = AudioEditOperation::InsertSilence { frames: 1000 };
        let longer = apply(&view, insert, 6000, 6001).unwrap();
        assert_eq!((longer.lead, longer.audio.frames()), (0, 49_000));
        assert_eq!(&longer.audio.samples()[..12_000], &samples[..12_000]);
        assert!(
            longer.audio.samples()[12_000..14_000]
                .iter()
                .all(|s| *s == 0.0)
        );
        assert_eq!(&longer.audio.samples()[14_000..], &samples[12_000..]);
        assert!(apply(&view, AudioEditOperation::InsertSilence { frames: 0 }, 0, 1).is_err());
        // Growing past two minutes is refused before any allocation.
        let too_long = AudioEditOperation::InsertSilence {
            frames: 120 * 48_000,
        };
        assert!(
            apply(&view, too_long, 0, 1)
                .unwrap_err()
                .contains("two minutes")
        );
    }

    #[test]
    fn audio_edit_selection_stretch_splices_exact_lengths_and_keeps_the_rest() {
        let view = take();
        let samples = view.samples();
        let stretch = AudioEditOperation::Stretch {
            ratio: 1.25,
            semitones: 0.0,
            quality: Default::default(),
            formants: false,
        };
        let edited = apply(&view, stretch, 9600, 33_600).unwrap();
        assert_eq!(edited.audio.frames(), 9600 + 30_000 + 14_400);
        assert_eq!(&edited.audio.samples()[..19_200], &samples[..19_200]);
        assert_eq!(&edited.audio.samples()[79_200..], &samples[67_200..]);
        let pitched = AudioEditOperation::Stretch {
            ratio: 1.0,
            semitones: 5.0,
            quality: Default::default(),
            formants: false,
        };
        let edited = apply(&view, pitched, 9600, 33_600).unwrap();
        assert_eq!(edited.audio.frames(), 48_000);
        assert_eq!(&edited.audio.samples()[..19_200], &samples[..19_200]);
        assert_eq!(&edited.audio.samples()[67_200..], &samples[67_200..]);
        assert_ne!(
            &edited.audio.samples()[19_200..67_200],
            &samples[19_200..67_200]
        );
        let huge = AudioBuffer::from_interleaved(48_000, 2, vec![0.1; 2 * 48_000 * 40]);
        let quadruple = AudioEditOperation::Stretch {
            ratio: 4.0,
            semitones: 0.0,
            quality: Default::default(),
            formants: false,
        };
        assert!(
            apply(&huge, quadruple, 0, 48_000 * 40)
                .unwrap_err()
                .contains("two minutes")
        );
    }

    #[test]
    fn audio_edit_every_operation_is_deterministic_and_preserves_the_outside() {
        let view = take();
        let samples = view.samples();
        let mut clicked = samples.to_vec();
        clicked[2 * 20_000] += 0.5;
        clicked[2 * 30_000 + 1] -= 0.4;
        let clicked = AudioBuffer::from_interleaved(48_000, 2, clicked);
        for operation in [
            AudioEditOperation::RemoveDc,
            AudioEditOperation::Declick { strength: 0.5 },
            AudioEditOperation::ReduceNoise {
                reduction_db: 12.0,
                profile: None,
            },
            AudioEditOperation::ReduceNoise {
                reduction_db: 12.0,
                profile: Some(AudioEditRange {
                    start_frame: 0,
                    end_frame: 4800,
                }),
            },
            AudioEditOperation::Gain { db: 3.0 },
            AudioEditOperation::NormalizeTo {
                target_db: -3.0,
                mode: AudioNormalizeMode::Peak,
            },
            AudioEditOperation::NormalizeTo {
                target_db: -20.0,
                mode: AudioNormalizeMode::Rms,
            },
            AudioEditOperation::Fade {
                direction: AudioFadeDirection::Out,
                shape: AudioFadeShape::SCurve,
            },
            AudioEditOperation::SwapChannels,
            AudioEditOperation::MonoSum,
            AudioEditOperation::InvertPhase {
                channels: AudioEditChannels::Left,
            },
        ] {
            let input = if matches!(operation, AudioEditOperation::Declick { .. }) {
                &clicked
            } else {
                &view
            };
            let edited = apply(input, operation, 12_000, 36_000).unwrap();
            let again = apply(input, operation, 12_000, 36_000).unwrap();
            let (out, before) = (edited.audio.samples(), input.samples());
            assert_eq!(edited.lead, 0, "{operation:?}");
            assert_eq!(out.len(), before.len(), "{operation:?}");
            assert!(
                out.iter()
                    .zip(again.audio.samples())
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "{operation:?}"
            );
            assert_eq!(&out[..24_000], &before[..24_000], "{operation:?}");
            assert_eq!(&out[72_000..], &before[72_000..], "{operation:?}");
            assert_ne!(
                &out[24_000..72_000],
                &before[24_000..72_000],
                "{operation:?}"
            );
        }
        // Clean audio gives the de-clicker nothing to do: no empty undo step.
        let refusal = apply(
            &view,
            AudioEditOperation::Declick { strength: 0.5 },
            0,
            48_000,
        );
        assert!(refusal.unwrap_err().contains("No clicks"));
        assert!(
            apply(&view, AudioEditOperation::Gain { db: 12.0 }, 0, 48_000)
                .unwrap_err()
                .contains("would clip")
        );
    }
}

#[cfg(test)]
mod project_pan_qa {
    use super::*;
    #[test]
    fn editor_balance_pcm_matches_the_project_law_without_changing_the_source() {
        let mut pool = SamplePool::new();
        let audio = AudioBuffer::from_interleaved(
            960,
            2,
            vec![0.25, -0.5, 0.25, -0.5, 0.25, -0.5, 0.25, -0.5],
        );
        pool.insert(windfall_project::SampleId(3), audio.clone());
        let mut clip = super::tests::clip();
        if let ClipContent::Audio { pan, .. } = &mut clip.content {
            *pan = 0.5;
        }
        for (law, gains) in [(PanLaw::Balance, (0.5, 1.0)), (PanLaw::Linear, (0.5, 1.5))] {
            let pcm = render_view_with_law(&pool, &clip, 60.0, law).unwrap();
            for &frame in pcm.samples().as_chunks::<2>().0 {
                assert_eq!(frame, [0.25 * gains.0, -0.5 * gains.1]);
            }
        }
        assert_eq!(
            pool.get(windfall_project::SampleId(3)).unwrap().samples(),
            audio.samples()
        );
    }
}
