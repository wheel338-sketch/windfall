//! Actual CPU algorithms; worker-only, bounded to two minutes / mono/stereo.
//! Spectral polyphony is a heuristic for sparse tonal audio, not source separation.
use crate::pitch::{PitchAnalyzer, PitchConfig, PitchError, PitchLimits, PitchSource};
use crate::{AnalysisError, AudioShape, FrameRange, Result, Work};
use rustfft::{FftPlanner, num_complex::Complex};
use windfall_core::AudioBuffer;
use windfall_project::{
    AudioAnalysisData, AudioDenoiseSettings, AudioEditRecipe, AudioWarpMarker, DetectedAudioNote,
    MAX_AUDIO_NOTES, MAX_WARP_MARKERS,
};
use windfall_stretch::{Quality, stretch_with_formants_cancellable};

fn invalid(message: &'static str) -> AnalysisError {
    AnalysisError::Invalid(message)
}
fn checked(audio: &AudioBuffer, work: &mut Work) -> Result<()> {
    if !matches!(audio.channels(), 1 | 2)
        || !(8000..=192000).contains(&audio.sample_rate())
        || audio.frames() == 0
        || audio.duration_secs() > 120.0
        || audio.samples().len() > 16 * 1024 * 1024
    {
        return Err(invalid(
            "classical audio limit: 120 seconds, mono/stereo, 64 MiB",
        ));
    }
    for chunk in audio.samples().chunks(4096) {
        work.checkpoint(chunk.len() as u64)?;
        if chunk.iter().any(|v| !v.is_finite()) {
            return Err(invalid("nonfinite audio"));
        }
    }
    Ok(())
}
fn hz_key(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}
fn strongest(audio: &AudioBuffer, work: &mut Work) -> Result<usize> {
    let channels = audio.channels() as usize;
    if channels == 1 {
        return Ok(0);
    }
    let mut energies = [0.0_f64; 2];
    for chunk in audio.samples().chunks(4096) {
        work.checkpoint(chunk.len() as u64)?;
        for frame in chunk.chunks_exact(channels) {
            for ch in 0..channels {
                energies[ch] += f64::from(frame[ch]).powi(2);
            }
        }
    }
    Ok(usize::from(energies[1] > energies[0]))
}

/// RMS novelty with a silence gate and 40 ms refractory period. No phase
/// cancellation: the strongest channel is selected for the whole analysis.
pub fn transients(audio: &AudioBuffer, work: &mut Work) -> Result<Vec<u32>> {
    work.require_memory(crate::add(audio.retained_bytes(), 2 * 1024 * 1024)?)?;
    checked(audio, work)?;
    let hop = (audio.sample_rate() as usize / 200).max(1);
    let channel = strongest(audio, work)?;
    let channels = audio.channels() as usize;
    let mut out = Vec::new();
    let (mut previous, mut last) = (0.0_f64, 0_usize);
    for start in (0..audio.frames()).step_by(hop) {
        let end = (start + hop).min(audio.frames());
        let rms = ((start..end)
            .map(|f| f64::from(audio.samples()[f * channels + channel]).powi(2))
            .sum::<f64>()
            / (end - start) as f64)
            .sqrt();
        work.checkpoint((end - start) as u64)?;
        if start > 0
            && rms > 0.008
            && rms > previous.max(0.002) * 1.8
            && start.saturating_sub(last) >= hop * 8
        {
            if out.len() >= MAX_WARP_MARKERS - 2 {
                return Err(AnalysisError::Budget("transients"));
            }
            out.push(start as u32);
            last = start;
        }
        previous = rms;
    }
    Ok(out)
}

pub fn detect(audio: &AudioBuffer, polyphonic: bool, work: &mut Work) -> Result<AudioAnalysisData> {
    work.require_memory(crate::add(
        audio.retained_bytes(),
        crate::classical_memory::detection(audio)?,
    )?)?;
    checked(audio, work)?;
    let onsets = transients(audio, work)?;
    let notes = if polyphonic {
        spectral_notes(audio, &onsets, work)?
    } else {
        mono_notes(audio, &onsets, work)?
    };
    let data = AudioAnalysisData {
        frames: audio.frames() as u32,
        sample_rate: audio.sample_rate(),
        polyphonic,
        notes,
        transients: onsets,
    };
    data.check()
        .map_err(|_| invalid("invalid detected notes"))?;
    Ok(data)
}
fn mono_notes(
    audio: &AudioBuffer,
    onsets: &[u32],
    work: &mut Work,
) -> Result<Vec<DetectedAudioNote>> {
    let config = PitchConfig {
        sample_rate: audio.sample_rate(),
        hop_frames: (audio.sample_rate() / 100).max(1),
        ..PitchConfig::default()
    };
    let mut analyzer = PitchAnalyzer::prepare(config, PitchLimits::default(), work)
        .map_err(|e| AnalysisError::Adapter(e.to_string()))?;
    let analysis = match analyzer.analyze(
        PitchSource {
            pcm: audio.samples(),
            shape: AudioShape::of(audio),
            range: FrameRange {
                start: 0,
                end: audio.frames() as u64,
            },
            frame_origin: 0,
        },
        work,
    ) {
        Ok(analysis) => analysis,
        // A short clip can still use warp/noise tools; never fabricate a pitch
        // from less than the detector's calibrated support window.
        Err(PitchError::TooShort { .. }) => return Ok(Vec::new()),
        Err(error) => return Err(AnalysisError::Adapter(error.to_string())),
    };
    let mut out: Vec<DetectedAudioNote> = Vec::new();
    let mut count = 0_u32;
    for cell in analysis.estimates() {
        let Some(hz) = cell.f0_hz else {
            count = 0;
            continue;
        };
        let key = hz_key(hz);
        if !(0.0..=127.0).contains(&key) {
            count = 0;
            continue;
        }
        let start = cell.range.start as u32;
        let end = cell.range.end as u32;
        let split = out.last().is_none_or(|n| {
            count == 0
                || n.end_frame != start
                || (n.midi_pitch.round() - key.round()).abs() >= 1.0
                || onsets.iter().any(|o| *o >= start && *o < end)
        });
        if split {
            if out.len() >= MAX_AUDIO_NOTES {
                return Err(AnalysisError::Budget("detected notes"));
            }
            out.push(DetectedAudioNote {
                start_frame: start,
                end_frame: end,
                midi_pitch: key,
                confidence: cell.confidence,
                rms: cell.rms,
            });
            count = 1;
        } else if let Some(n) = out.last_mut() {
            count += 1;
            let mix = 1.0 / f64::from(count);
            n.midi_pitch += (key - n.midi_pitch) * mix;
            n.confidence += (cell.confidence - n.confidence) * mix;
            n.rms += (cell.rms - n.rms) * mix;
            n.end_frame = end;
        }
    }
    // Remove isolated tracking cells, preserving calibrated held/repeated notes.
    out.retain(|n| n.end_frame - n.start_frame >= audio.sample_rate() / 40);
    Ok(out)
}

fn spectral_notes(
    audio: &AudioBuffer,
    onsets: &[u32],
    work: &mut Work,
) -> Result<Vec<DetectedAudioNote>> {
    let rate = audio.sample_rate() as usize;
    let n = (rate / 10).next_power_of_two().clamp(1024, 16384);
    let hop = (rate / 100).max(1);
    let channels = audio.channels() as usize;
    let channel = strongest(audio, work)?;
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut bins = vec![Complex::default(); n];
    let mut scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
    let mut magnitudes = vec![0.0_f64; n / 2 + 1];
    let mut active: [Option<usize>; 128] = [None; 128];
    let mut notes: Vec<DetectedAudioNote> = Vec::new();
    let mut counts = Vec::<u32>::new();
    for center in (0..audio.frames()).step_by(hop) {
        work.checkpoint((n * (n.ilog2() as usize + 1) * 20) as u64)?;
        for (i, bin) in bins.iter_mut().enumerate() {
            let frame = center as isize + i as isize - n as isize / 2;
            let value = if frame >= 0 && frame < audio.frames() as isize {
                f64::from(audio.samples()[frame as usize * channels + channel])
            } else {
                0.0
            };
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            *bin = Complex::new(value * window, 0.0);
        }
        // Gate and dynamics use the current 10 ms cell, not the lookahead FFT
        // window. Future onsets must not create early notes; past tails must
        // not extend offsets through actual silence.
        let end_frame = (center + hop).min(audio.frames());
        let rms = ((center..end_frame)
            .map(|frame| f64::from(audio.samples()[frame * channels + channel]).powi(2))
            .sum::<f64>()
            / (end_frame - center) as f64)
            .sqrt();
        fft.process_with_scratch(&mut bins, &mut scratch);
        for (m, b) in magnitudes.iter_mut().zip(&bins) {
            *m = b.norm();
        }
        let peak = magnitudes.iter().copied().fold(0.0_f64, f64::max);
        let mut candidates = Vec::new();
        if rms >= 0.003 {
            for k in 2..n / 2 - 1 {
                if magnitudes[k] < peak * 0.12
                    || magnitudes[k] <= magnitudes[k - 1]
                    || magnitudes[k] < magnitudes[k + 1]
                {
                    continue;
                }
                let [a, b, c] = [magnitudes[k - 1], magnitudes[k], magnitudes[k + 1]]
                    .map(|v| v.max(1e-20).ln());
                let curvature = a - 2.0 * b + c;
                let offset = if curvature < -1e-12 {
                    (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
                } else {
                    0.0
                };
                let hz = (k as f64 + offset) * rate as f64 / n as f64;
                if (50.0..=1500.0).contains(&hz) {
                    candidates.push((hz, magnitudes[k] / peak));
                }
            }
        }
        // Ascending frequency; suppress integer harmonics of accepted lower
        // peaks. This intentionally cannot resolve octave-unison instruments.
        let mut roots: Vec<f64> = Vec::new();
        let mut present = [None; 128];
        for (hz, salience) in candidates {
            if roots.iter().any(|root| {
                let multiple = (hz / root).round();
                multiple >= 2.0 && (1200.0 * (hz / (root * multiple)).log2()).abs() < 35.0
            }) {
                continue;
            }
            let key = hz_key(hz);
            if !(0.0..=127.0).contains(&key) || roots.len() >= 8 {
                continue;
            }
            roots.push(hz);
            present[key.round() as usize] = Some((key, salience));
        }
        let end = (center + hop).min(audio.frames()) as u32;
        let onset = onsets.iter().any(|o| *o >= center as u32 && *o < end);
        for key in 0..128 {
            if let Some((pitch, salience)) = present[key] {
                if active[key].is_none() || onset {
                    if notes.len() >= MAX_AUDIO_NOTES {
                        return Err(AnalysisError::Budget("polyphonic notes"));
                    }
                    active[key] = Some(notes.len());
                    notes.push(DetectedAudioNote {
                        start_frame: center as u32,
                        end_frame: end,
                        midi_pitch: pitch,
                        confidence: salience,
                        rms,
                    });
                    counts.push(1);
                } else if let Some(index) = active[key] {
                    counts[index] += 1;
                    let mix = 1.0 / f64::from(counts[index]);
                    let note = &mut notes[index];
                    note.midi_pitch += (pitch - note.midi_pitch) * mix;
                    note.confidence += (salience - note.confidence) * mix;
                    note.end_frame = end;
                    note.rms = note.rms.max(rms);
                }
            } else {
                active[key] = None;
            }
        }
    }
    notes.retain(|n| n.end_frame - n.start_frame >= audio.sample_rate() / 20);
    notes.sort_by_key(|n| (n.start_frame, n.midi_pitch.round() as u8));
    Ok(notes)
}

/// Learned channel power spectra, square-root Hann overlap-add, Wiener-style
/// gains, spectral floor and wet/dry. Source/profile PCM is never modified.
pub fn denoise(
    audio: &AudioBuffer,
    settings: AudioDenoiseSettings,
    work: &mut Work,
) -> Result<AudioBuffer> {
    checked(audio, work)?;
    if settings.profile_start >= settings.profile_end
        || settings.profile_end as usize > audio.frames()
        || !settings.reduction_db.is_finite()
        || !(0.0..=40.0).contains(&settings.reduction_db)
        || !settings.wet.is_finite()
        || !(0.0..=1.0).contains(&settings.wet)
    {
        return Err(invalid("noise profile / attenuation / wet"));
    }
    let n = (audio.sample_rate() as usize / 40)
        .next_power_of_two()
        .clamp(256, 8192);
    if (settings.profile_end - settings.profile_start) < n as u32 {
        return Err(invalid(
            "noise-only profile must contain at least one FFT window (25-43 ms)",
        ));
    }
    if settings.wet == 0.0 || settings.reduction_db == 0.0 {
        work.require_memory(audio.retained_bytes())?;
        return Ok(audio.clone());
    }
    work.require_memory(crate::add(
        audio.retained_bytes(),
        crate::classical_memory::denoising(audio)?,
    )?)?;
    let channels = audio.channels() as usize;
    let hop = n / 4;
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);
    let mut scratch = vec![
        Complex::default();
        forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
    ];
    let window: Vec<f64> = (0..n)
        .map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / n as f64).cos()).sqrt())
        .collect();
    let mut bins = vec![Complex::default(); n];
    let mut noise = vec![vec![0.0; n]; channels];
    let mut windows = 0;
    for start in (settings.profile_start as usize..=settings.profile_end as usize - n).step_by(hop)
    {
        work.checkpoint((n * (n.ilog2() as usize + 1) * channels * 20) as u64)?;
        for (ch, powers) in noise.iter_mut().enumerate() {
            for i in 0..n {
                bins[i] = Complex::new(
                    f64::from(audio.samples()[(start + i) * channels + ch]) * window[i],
                    0.0,
                );
            }
            forward.process_with_scratch(&mut bins, &mut scratch);
            for (power, bin) in powers.iter_mut().zip(&bins) {
                *power += bin.norm_sqr();
            }
        }
        windows += 1;
    }
    for powers in &mut noise {
        for power in powers {
            *power /= windows as f64;
        }
    }
    let mut output = vec![0.0_f64; audio.samples().len()];
    let mut weights = vec![0.0_f64; audio.frames()];
    let floor = 10.0_f64.powf(-settings.reduction_db / 20.0);
    for center in (0..audio.frames() + n / 2).step_by(hop) {
        work.checkpoint((n * (n.ilog2() as usize + 1) * channels * 40) as u64)?;
        let start = center as isize - n as isize / 2;
        for (ch, powers) in noise.iter().enumerate() {
            for i in 0..n {
                let f = start + i as isize;
                bins[i] = Complex::new(
                    if f >= 0 && f < audio.frames() as isize {
                        f64::from(audio.samples()[f as usize * channels + ch]) * window[i]
                    } else {
                        0.0
                    },
                    0.0,
                );
            }
            forward.process_with_scratch(&mut bins, &mut scratch);
            for (bin, power) in bins.iter_mut().zip(powers) {
                let signal = bin.norm_sqr();
                let gain = (1.0 - power * 1.15 / signal.max(1e-30))
                    .max(0.0)
                    .sqrt()
                    .max(floor);
                *bin *= gain;
            }
            inverse.process_with_scratch(&mut bins, &mut scratch);
            for i in 0..n {
                let f = start + i as isize;
                if f >= 0 && f < audio.frames() as isize {
                    output[f as usize * channels + ch] += bins[i].re * window[i] / n as f64;
                    if ch == 0 {
                        weights[f as usize] += window[i] * window[i];
                    }
                }
            }
        }
    }
    let samples: Vec<f32> = output
        .iter()
        .enumerate()
        .map(|(i, wet)| {
            let dry = f64::from(audio.samples()[i]);
            ((wet / weights[i / channels].max(1e-20)) * settings.wet + dry * (1.0 - settings.wet))
                as f32
        })
        .collect();
    for chunk in samples.chunks(4096) {
        work.checkpoint(chunk.len() as u64)?;
        if chunk.iter().any(|v| !v.is_finite()) {
            return Err(invalid("nonfinite denoise output"));
        }
    }
    Ok(AudioBuffer::from_interleaved(
        audio.sample_rate(),
        audio.channels(),
        samples,
    ))
}

fn mapped(markers: &[AudioWarpMarker], source: u32) -> u32 {
    let pair = markers
        .windows(2)
        .find(|w| source <= w[1].source_frame)
        .expect("checked endpoint");
    let part = (source - pair[0].source_frame) as f64
        / (pair[1].source_frame - pair[0].source_frame) as f64;
    (pair[0].target_frame as f64 + part * (pair[1].target_frame - pair[0].target_frame) as f64)
        .round() as u32
}

/// Piecewise prepared spectral rendering. The exact output is reused for
/// audition, playlist playback and export; nothing runs on an audio callback.
pub fn render(
    audio: &AudioBuffer,
    recipe: &AudioEditRecipe,
    work: &mut Work,
) -> Result<AudioBuffer> {
    checked(audio, work)?;
    recipe.check().map_err(AnalysisError::Adapter)?;
    work.require_memory(crate::add(
        audio.retained_bytes(),
        crate::classical_memory::rendering(audio, recipe)?,
    )?)?;
    if audio.frames() != recipe.analysis.frames as usize
        || audio.sample_rate() != recipe.analysis.sample_rate
    {
        return Err(AnalysisError::Stale);
    }
    let source = if let Some(d) = recipe.denoise {
        denoise(audio, d, work)?
    } else {
        audio.clone()
    };
    let channels = audio.channels() as usize;
    let frames = recipe.output_frames().map_err(AnalysisError::Adapter)? as usize;
    if frames * channels > 16 * 1024 * 1024 {
        return Err(AnalysisError::Budget("render output"));
    }
    let points: Vec<(u32, u32)> = recipe
        .render_markers()
        .map_err(AnalysisError::Adapter)?
        .iter()
        .map(|m| (m.source_frame, m.target_frame))
        .collect();
    // Avoid phase preparation and all seam arithmetic for a neutral recipe.
    if recipe
        .overrides
        .iter()
        .all(|edit| edit.midi_pitch == recipe.analysis.notes[edit.note as usize].midi_pitch)
        && points.iter().all(|(source, target)| source == target)
    {
        work.check()?;
        return Ok(source);
    }
    let mut result = vec![0.0; frames * channels];
    // Each independently transformed interval receives one full analysis
    // window of source context. Its core stays at the exact anchor frames.
    // Linear 8 ms crossfades bridge independently reconstructed phases;
    // widths shrink to a quarter of either neighbouring interval.
    let context = (audio.sample_rate() as usize * 120 / 1000).max(1);
    let fade_limit = (audio.sample_rate() as usize * 8 / 1000).max(1);
    let mut previous_tail = Vec::<f32>::new();
    let mut previous_core = 0_usize;
    for pair in points.windows(2) {
        work.checkpoint(4096)?;
        let (a, b) = (pair[0], pair[1]);
        if a.0 >= b.0 || a.1 >= b.1 {
            return Err(invalid("combined note/warp anchors cross"));
        }
        let ratio = (b.1 - a.1) as f64 / (b.0 - a.0) as f64;
        if !(0.25..=4.0).contains(&ratio) {
            return Err(invalid("combined warp interval ratio"));
        }
        let shift = recipe
            .overrides
            .iter()
            .find_map(|edit| {
                let note = &recipe.analysis.notes[edit.note as usize];
                (a.0 >= note.start_frame && b.0 <= note.end_frame)
                    .then_some(edit.midi_pitch - note.midi_pitch)
            })
            .unwrap_or(0.0);
        let first = (a.0 as usize).saturating_sub(context);
        let last = (b.0 as usize + context).min(source.frames());
        let core = (b.1 - a.1) as usize;
        let core_offset = (((a.0 as usize - first) as f64) * ratio).round() as usize;
        let buffer = AudioBuffer::from_interleaved(
            audio.sample_rate(),
            audio.channels(),
            source.samples()[first * channels..last * channels].to_vec(),
        );
        let mut failure = None;
        let rendered = stretch_with_formants_cancellable(
            &buffer,
            ratio,
            shift,
            Quality::High,
            false,
            &mut || match work.checkpoint(4096 * 64) {
                Ok(()) => true,
                Err(e) => {
                    failure = Some(e);
                    false
                }
            },
        );
        if let Some(error) = failure {
            return Err(error);
        }
        let rendered = rendered.ok_or(AnalysisError::Cancelled)?;
        if rendered.frames() < core_offset + core {
            return Err(invalid("stretch context length mismatch"));
        }
        result[a.1 as usize * channels..b.1 as usize * channels].copy_from_slice(
            &rendered.samples()[core_offset * channels..(core_offset + core) * channels],
        );
        let blend = fade_limit
            .min(previous_core / 4)
            .min(core / 4)
            .min(previous_tail.len() / channels)
            .min(core_offset);
        let boundary = a.1 as usize;
        for i in 0..blend * 2 {
            let frame = boundary - blend + i;
            let weight = (i + 1) as f64 / (blend * 2 + 1) as f64;
            for ch in 0..channels {
                let left = if i < blend {
                    result[frame * channels + ch]
                } else {
                    previous_tail[(i - blend) * channels + ch]
                };
                let right = rendered.samples()[(core_offset - blend + i) * channels + ch];
                result[frame * channels + ch] =
                    (f64::from(left) * (1.0 - weight) + f64::from(right) * weight) as f32;
            }
        }
        let tail = (rendered.frames() - core_offset - core).min(fade_limit);
        previous_tail = rendered.samples()
            [(core_offset + core) * channels..(core_offset + core + tail) * channels]
            .to_vec();
        previous_core = core;
    }
    for chunk in result.chunks(4096) {
        work.checkpoint(chunk.len() as u64)?;
        if chunk.iter().any(|v| !v.is_finite()) {
            return Err(invalid("nonfinite render"));
        }
    }
    Ok(AudioBuffer::from_interleaved(
        audio.sample_rate(),
        audio.channels(),
        result,
    ))
}

/// Review notes in output frame coordinates, with perceptual RMS-derived
/// velocity. MIDI ticks are resolved by the host's tempo clock at Apply.
pub fn reviewed_notes(recipe: &AudioEditRecipe) -> Result<Vec<DetectedAudioNote>> {
    recipe.check().map_err(AnalysisError::Adapter)?;
    let markers = recipe.render_markers().map_err(AnalysisError::Adapter)?;
    Ok(recipe
        .analysis
        .notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut note = n.clone();
            if let Some(edit) = recipe.overrides.iter().find(|o| o.note as usize == i) {
                note.start_frame = edit.start_frame;
                note.end_frame = edit.end_frame;
                note.midi_pitch = edit.midi_pitch;
            } else {
                note.start_frame = mapped(&markers, n.start_frame);
                note.end_frame = mapped(&markers, n.end_frame);
            }
            note
        })
        .collect())
}
