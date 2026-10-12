//! Deterministic control/worker-side clip slicing. No source audio is changed.
//! Shared by the native shell and WebAssembly; never run on an audio callback.
use serde::{Deserialize, Serialize};
use windfall_core::AudioBuffer;

use crate::{Clip, ClipContent, ClipInit, ClipStretch, Command, MAX_SONG_TICKS, PPQ};

pub const MAX_MARKERS: usize = 2048;
const OVERVIEW_BUCKETS: usize = 128;
const MAX_ANALYSIS_FRAMES: usize = 32_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum SliceOptions {
    /// Song-aligned grid, expressed in ticks per division.
    #[serde(rename_all = "camelCase")]
    Grid { grid_ticks: u32 },
    /// 0 selects strong attacks; 1 includes weaker attacks. Not beat estimation.
    Transients { sensitivity: f32 },
    /// Read authored WAV regions in the native shell, without detecting attacks.
    WavMarkers,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SliceMarker {
    /// Interior boundary relative to the visible clip, rounded to a tick.
    pub tick: u32,
    pub strength: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SliceAnalysis {
    pub markers: Vec<SliceMarker>,
    /// Absolute peaks of the visible audio, in playback order (including reverse).
    pub peaks: Vec<f32>,
}

/// These combinations cannot preserve the original time/envelope by offsets alone.
pub fn supported(clip: &Clip, tempo: f64, swing: f32) -> Result<(), String> {
    if !tempo.is_finite() || !(crate::MIN_TEMPO_BPM..=crate::MAX_TEMPO_BPM).contains(&tempo) {
        return Err("Invalid project tempo.".into());
    }
    if swing != 0.0 {
        return Err("Set project swing to zero before slicing audio.".into());
    }
    let ClipContent::Audio {
        fade_in,
        fade_out,
        stretch,
        pitch,
        ..
    } = clip.content
    else {
        return Err("Select one playlist audio clip to slice.".into());
    };
    if fade_in != 0 || fade_out != 0 {
        return Err(
            "Remove the clip fades before slicing; slice envelopes are not supported yet.".into(),
        );
    }
    if stretch != ClipStretch::Tape {
        return Err(
            "Use tape mode before slicing; spectral clip slicing is not supported yet.".into(),
        );
    }
    if !pitch.is_finite() || !(-48.0..=48.0).contains(&pitch) {
        return Err("Invalid tape pitch.".into());
    }
    if clip.length == 0
        || clip
            .start
            .checked_add(clip.length)
            .is_none_or(|end| end > MAX_SONG_TICKS)
        || clip
            .offset
            .checked_add(clip.length)
            .is_none_or(|end| end > MAX_SONG_TICKS)
    {
        return Err("The clip's timeline or source offset is out of range.".into());
    }
    Ok(())
}

/// Analyze only the visible source region. Uses channel energy rather than a
/// mono sum so opposing stereo polarities do not hide an attack. An analysis
/// window is roughly 5 ms in playback time. Thresholds are relative to the
/// strongest positive energy rise; candidates have a 30 ms refractory period.
pub fn analyze(
    audio: &AudioBuffer,
    clip: &Clip,
    tempo: f64,
    swing: f32,
    options: SliceOptions,
) -> Result<SliceAnalysis, String> {
    supported(clip, tempo, swing)?;
    match options {
        SliceOptions::WavMarkers => {
            return Err("WAV slice markers require a native source file.".into());
        }
        SliceOptions::Grid { grid_ticks }
            if ![PPQ / 4, PPQ / 2, PPQ, PPQ * 2, PPQ * 4].contains(&grid_ticks) =>
        {
            return Err("Choose a sixteenth, eighth, quarter, half, or whole-note grid.".into());
        }
        SliceOptions::Transients { sensitivity }
            if !sensitivity.is_finite() || !(0.0..=1.0).contains(&sensitivity) =>
        {
            return Err("Sensitivity must be between 0 and 1.".into());
        }
        _ => {}
    }
    let ClipContent::Audio { reverse, pitch, .. } = clip.content else {
        unreachable!()
    };
    let ticks_per_second = tempo * f64::from(PPQ) / 60.0;
    let frames_per_tick =
        f64::from(audio.sample_rate()) * 2.0_f64.powf(f64::from(pitch) / 12.0) / ticks_per_second;
    let natural_ticks = audio.frames() as f64 / frames_per_tick;
    // Imported clips ceil their natural duration to a tick. Allow that last
    // fraction of a tick, but reject deliberate silent tails or empty sources.
    if audio.frames() == 0
        || f64::from(clip.offset) >= natural_ticks
        || f64::from(clip.offset + clip.length) > natural_ticks.ceil()
    {
        return Err("Trim the clip to its available source audio before slicing.".into());
    }
    let first = (f64::from(clip.offset) * frames_per_tick).floor() as usize;
    let last = ((f64::from(clip.offset + clip.length) * frames_per_tick).ceil() as usize)
        .min(audio.frames());
    let frames = last - first;
    if frames > MAX_ANALYSIS_FRAMES {
        return Err(
            "Trim the visible clip to at most 32 million source frames before analysis.".into(),
        );
    }
    let channels = usize::from(audio.channels());
    let window_frames =
        (f64::from(audio.sample_rate()) * 2.0_f64.powf(f64::from(pitch) / 12.0) * 0.005)
            .round()
            .max(1.0) as usize;
    let mut energy = vec![0.0_f64; frames.div_ceil(window_frames)];
    let mut counts = vec![0_usize; energy.len()];
    let mut peaks = vec![0.0_f32; OVERVIEW_BUCKETS];
    for local in 0..frames {
        let source = if reverse {
            audio.frames() - 1 - first - local
        } else {
            first + local
        };
        let mut square = 0.0;
        let mut peak = 0.0_f32;
        for value in &audio.samples()[source * channels..(source + 1) * channels] {
            if !value.is_finite() {
                return Err("The source contains non-finite audio samples.".into());
            }
            square += f64::from(*value).powi(2);
            peak = peak.max(value.abs());
        }
        let window = local / window_frames;
        energy[window] += square / channels as f64;
        counts[window] += 1;
        let bucket = local * OVERVIEW_BUCKETS / frames;
        peaks[bucket] = peaks[bucket].max(peak.min(1.0));
    }
    for (energy, count) in energy.iter_mut().zip(counts) {
        *energy = (*energy / count as f64).sqrt();
    }
    let markers = match options {
        SliceOptions::WavMarkers => {
            return Err("WAV slice markers require a native source file.".into());
        }
        SliceOptions::Grid { grid_ticks } => {
            let first = grid_ticks - clip.start % grid_ticks;
            let count = clip.length.saturating_sub(first).div_ceil(grid_ticks) as usize;
            if count > MAX_MARKERS {
                return Err("Too many markers. Choose a coarser grid or trim the clip.".into());
            }
            (first..clip.length)
                .step_by(grid_ticks as usize)
                .map(|tick| SliceMarker {
                    tick,
                    strength: 1.0,
                })
                .collect()
        }
        SliceOptions::Transients { sensitivity } => {
            let rises: Vec<f64> = energy.windows(2).map(|e| (e[1] - e[0]).max(0.0)).collect();
            let maximum = rises.iter().copied().fold(0.0_f64, f64::max);
            let threshold = maximum * (0.65 - f64::from(sensitivity) * 0.60);
            let spacing = (ticks_per_second * 0.030).ceil().max(1.0) as u32;
            let mut markers = Vec::new();
            if maximum > 1.0e-6 {
                for (index, rise) in rises.iter().enumerate() {
                    if *rise < threshold || *rise < 1.0e-6 {
                        continue;
                    }
                    let tick =
                        (((index + 1) * window_frames) as f64 / frames_per_tick).round() as u32;
                    if tick == 0
                        || tick >= clip.length
                        || markers
                            .last()
                            .is_some_and(|m: &SliceMarker| tick.saturating_sub(m.tick) < spacing)
                    {
                        continue;
                    }
                    markers.push(SliceMarker {
                        tick,
                        strength: (*rise / maximum) as f32,
                    });
                    if markers.len() > MAX_MARKERS {
                        return Err(
                            "Too many transients. Lower sensitivity or trim the clip.".into()
                        );
                    }
                }
            }
            markers
        }
    };
    Ok(SliceAnalysis { markers, peaks })
}

/// Validates every boundary before building a single undoable batch. Slices
/// keep the full immutable source, so reverse and pitched tape offsets continue
/// to address the same audio. The engine applies its normal 3 ms edge declick.
pub fn split_command(
    clip: &Clip,
    markers: &[u32],
    tempo: f64,
    swing: f32,
) -> Result<Command, String> {
    supported(clip, tempo, swing)?;
    if markers.is_empty() || markers.len() > MAX_MARKERS {
        return Err("Select between 1 and 2048 interior slice markers.".into());
    }
    let mut previous = 0;
    for tick in markers {
        if *tick <= previous || *tick >= clip.length {
            return Err("Slice markers must be ordered, unique, and inside the clip.".into());
        }
        previous = *tick;
    }
    let mut begin = 0;
    let clips = markers
        .iter()
        .copied()
        .chain(std::iter::once(clip.length))
        .map(|end| {
            let init = ClipInit {
                track: clip.track,
                start: clip.start + begin,
                length: Some(end - begin),
                offset: Some(clip.offset + begin),
                muted: Some(clip.muted),
                content: clip.content.clone(),
            };
            begin = end;
            init
        })
        .collect();
    Ok(Command::Batch {
        label: Some("Slice audio clip".into()),
        commands: vec![
            Command::RemoveClips {
                clips: vec![clip.id],
            },
            Command::AddClips { clips },
        ],
    })
}
