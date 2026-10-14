//! Native sample references and slicer records from DawVert flp_dec_plugins.py
//! at c6c8be9e (GPL-3.0-or-later). No sample file is opened by this module.
use windfall_dsp::granular::SliceRegion;
use windfall_project::{ChannelId, Command, Document, Project, SampleId, SamplePlayback};

#[derive(Debug, Clone, PartialEq)]
pub struct NativeSlice {
    pub start: u32,
    pub key: u8,
    pub reverse: bool,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRequest {
    pub channel: ChannelId,
    pub sample: SampleId,
    pub slices: Vec<NativeSlice>,
    pub pitch: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub base_key: u8,
}

pub(super) struct NativePlayback {
    pub path: Option<String>,
    pub playback: SamplePlayback,
    pub slices: Vec<NativeSlice>,
    pub base_key: u8,
    pub cue_source: bool,
}
fn word(b: &[u8], i: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn float(b: &[u8], i: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?)).filter(|v| v.is_finite())
}
fn string(b: &[u8], at: &mut usize) -> Option<String> {
    let len = usize::from(*b.get(*at)?);
    *at += 1;
    let text = String::from_utf8_lossy(b.get(*at..(*at).checked_add(len)?)?).into_owned();
    *at += len;
    Some(text)
}

pub(super) fn read(name: &str, b: &[u8]) -> Option<NativePlayback> {
    if name.trim().eq_ignore_ascii_case("Fruity Slicer") {
        if b.len() < 32 {
            return None;
        }
        let pitch = (word(b, 12)? as f32 / 100.0).clamp(-24.0, 24.0);
        let attack_ms = (word(b, 24)?.max(0) as f32 / 128.0 * 100.0).clamp(0.0, 1000.0);
        let release_ms = (word(b, 28)?.max(0) as f32 / 128.0 * 100.0).clamp(1.0, 1000.0);
        let mut playback = SamplePlayback {
            source_bpm: f64::from(float(b, 8).unwrap_or(120.0).clamp(10.0, 522.0)),
            source_beats: f64::from(float(b, 4).unwrap_or(4.0).clamp(0.0625, 256.0)),
            slices: vec![SliceRegion {
                pitch,
                attack_ms,
                release_ms,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut at = 32;
        let path = string(b, &mut at).filter(|s| !s.is_empty());
        let count = word(b, at).filter(|v| (0..=128).contains(v))? as usize;
        at += 4;
        let mut slices = Vec::new();
        let mut implicit = Vec::new();
        for i in 0..count {
            let name = string(b, &mut at)?;
            let start = word(b, at)? as u32;
            let key = word(b, at + 4)?;
            let reverse = *b.get(at + 12)? != 0;
            at += 13;
            slices.push(NativeSlice {
                start,
                key: if key == -1 {
                    (60 + i).min(127) as u8
                } else {
                    u8::try_from(key).ok().filter(|k| *k <= 127)?
                },
                reverse,
                name,
            });
            implicit.push(key == -1);
        }
        // Source animate byte, starting key u32, then play-to-end and UI
        // options. Preserve absolute custom keys; sequential keys use start.
        let base_key = word(b, at + 1)
            .and_then(|v| u8::try_from(v).ok())
            .filter(|k| *k <= 127)
            .unwrap_or(60);
        for (i, (slice, implicit)) in slices.iter_mut().zip(implicit).enumerate() {
            if implicit {
                slice.key = base_key.checked_add(i as u8).filter(|k| *k <= 127)?;
            }
            let mut end = slice.name.len().min(256);
            while !slice.name.is_char_boundary(end) {
                end -= 1;
            }
            slice.name.truncate(end);
        }
        if playback.source_bpm <= 0.0 {
            playback.source_bpm = 120.0;
        }
        return Some(NativePlayback {
            path,
            playback,
            slices,
            base_key,
            cue_source: false,
        });
    }
    if name.trim().eq_ignore_ascii_case("Slicex") && b.len() >= 40 {
        let mut at = 96;
        let path = string(b, &mut at).filter(|p| !p.is_empty());
        let embedded = word(b, at)
            .filter(|n| *n >= 0)
            .and_then(|n| b.get(at + 4..(at + 4).checked_add(n as usize)?));
        let mut playback = SamplePlayback {
            slices: vec![SliceRegion {
                pitch: (word(b, 24)? as f32 / 100.0).clamp(-24.0, 24.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        if let Some((source, starts)) = embedded.and_then(embedded_wave) {
            let frames = source.data.len();
            if !starts.is_empty() {
                let pitch = playback.slices[0].pitch;
                playback.slices = starts
                    .iter()
                    .enumerate()
                    .map(|(i, start)| SliceRegion {
                        start: *start as f32 / frames as f32,
                        end: starts
                            .get(i + 1)
                            .map_or(1.0, |next| *next as f32 / frames as f32),
                        pitch,
                        ..Default::default()
                    })
                    .collect();
            }
            playback.inline_source = Some(source);
        }
        return Some(NativePlayback {
            path,
            playback,
            slices: Vec::new(),
            base_key: 60,
            cue_source: name.trim().eq_ignore_ascii_case("Slicex"),
        });
    }
    if name.trim().eq_ignore_ascii_case("BassDrum") && b.get(..4) == Some(b"SSLF") {
        let len = word(b, 4)? as usize;
        let data = b.get(8..8usize.checked_add(len)?)?;
        let file = data.get(260..516)?;
        let end = file.iter().position(|c| *c == 0).unwrap_or(file.len());
        let path = String::from_utf8_lossy(&file[..end]).into_owned();
        return Some(NativePlayback {
            path: (!path.is_empty()).then_some(path),
            playback: SamplePlayback::default(),
            slices: Vec::new(),
            base_key: 60,
            cue_source: name.trim().eq_ignore_ascii_case("Slicex"),
        });
    }
    None
}

/// Original bounded RIFF reader for the public Slicex WAV payload. DawVert's
/// flp_dec_plugins.py uses WAV cue.sampleoffset (objects/file/audio_wav.py).
/// The current inline stand-in source is mono; stereo is downmixed explicitly.
fn embedded_wave(bytes: &[u8]) -> Option<(windfall_project::InlinePlaybackSource, Vec<u32>)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let length = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let bytes = bytes.get(..length.checked_add(8)?)?;
    let mut at = 12usize;
    let mut format = None;
    let mut data = None;
    let mut starts = Vec::new();
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let chunk = bytes.get(at + 8..(at + 8).checked_add(size)?)?;
        if id == b"fmt " && chunk.len() >= 16 {
            let short =
                |i: usize| u16::from_le_bytes(chunk[i..i + 2].try_into().expect("bounded fmt"));
            format = Some((
                short(0),
                short(2),
                u32::from_le_bytes(chunk[4..8].try_into().ok()?),
                short(12),
                short(14),
            ));
        } else if id == b"data" {
            data = Some(chunk);
        } else if id == b"cue " && chunk.len() >= 4 {
            let count = u32::from_le_bytes(chunk[..4].try_into().ok()?) as usize;
            if count > 128 || count > (chunk.len() - 4) / 24 {
                return None;
            }
            for i in 0..count {
                let p = &chunk[4 + i * 24..28 + i * 24];
                starts.push(u32::from_le_bytes(p[20..24].try_into().ok()?));
            }
        }
        at = (at + 8).checked_add(size)?.checked_add(size & 1)?;
    }
    let (tag, channels, rate, align, bits) = format?;
    let data = data?;
    let width = usize::from(bits / 8);
    let channels = usize::from(channels);
    if channels == 0
        || channels > 2
        || !(1000..=384000).contains(&rate)
        || !matches!((tag, bits), (1, 8 | 16 | 24 | 32) | (3, 32))
        || usize::from(align) != channels * width
        || !data.len().is_multiple_of(usize::from(align))
    {
        return None;
    }
    let frames = data.len() / usize::from(align);
    if frames == 0 || frames > 4 * 1024 * 1024 {
        return None;
    }
    let mut mono = Vec::with_capacity(frames);
    for frame in data.chunks_exact(usize::from(align)) {
        let mut sum = 0.0;
        for b in frame.chunks_exact(width) {
            sum += match (tag, bits) {
                (1, 8) => (f32::from(b[0]) - 128.0) / 128.0,
                (1, 16) => f32::from(i16::from_le_bytes(b.try_into().ok()?)) / 32768.0,
                (1, 24) => {
                    ((i32::from(b[0]) | (i32::from(b[1]) << 8) | (i32::from(b[2]) << 16)) << 8 >> 8)
                        as f32
                        / 8388608.0
                }
                (1, 32) => i32::from_le_bytes(b.try_into().ok()?) as f32 / 2147483648.0,
                (3, 32) => f32::from_le_bytes(b.try_into().ok()?),
                _ => return None,
            };
        }
        let value = sum / channels as f32;
        if !value.is_finite() {
            return None;
        }
        mono.push(value.clamp(-1.0, 1.0));
    }
    starts.sort_unstable();
    starts.dedup();
    starts.retain(|s| (*s as usize) < frames);
    if starts.len() > 128 {
        return None;
    }
    Some((
        windfall_project::InlinePlaybackSource {
            sample_rate: rate,
            data: mono,
        },
        starts,
    ))
}

/// Complete source-frame regions only after the shell has decoded the exact
/// sample. Tests can supply independently counted frame/channel metadata.
pub fn resolve_native_slices(
    project: &mut Project,
    requests: &[SliceRequest],
    metadata: &[(SampleId, usize, usize)],
) -> Vec<String> {
    let mut document = Document::new(project.clone());
    let mut warnings = Vec::new();
    for request in requests {
        let Some(&(_, frames, channels)) = metadata
            .iter()
            .find(|(id, _, _)| *id == request.sample)
            .filter(|(_, f, c)| *f > 0 && *c > 0)
        else {
            warnings.push(
                windfall_core::notice!("flp.slices.needMetadata", sample = request.sample.0)
                    .to_string(),
            );
            continue;
        };
        let Some(channel) = document.project().channel(request.channel) else {
            continue;
        };
        let mut asset = channel.sample_playback.clone().unwrap_or_default();
        let mut ordered: Vec<_> = request.slices.iter().collect();
        ordered.sort_by_key(|s| s.start);
        let base = ordered
            .iter()
            .map(|s| s.key)
            .min()
            .unwrap_or(request.base_key);
        let top = ordered.iter().map(|s| s.key).max().unwrap_or(base);
        let mut slices = vec![
            SliceRegion {
                gain: 0.0,
                ..Default::default()
            };
            usize::from(top - base) + 1
        ];
        for (i, s) in ordered.iter().enumerate() {
            // DawVert explicitly multiplies stored starts by channels/2.
            let start = f64::from(s.start) * channels as f64 / 2.0;
            let end = ordered.get(i + 1).map_or(frames as f64, |next| {
                f64::from(next.start) * channels as f64 / 2.0
            });
            if start >= end || end > frames as f64 {
                warnings.push(
                    windfall_core::notice!("flp.slices.badBoundaries", slice = s.name).to_string(),
                );
                continue;
            }
            slices[usize::from(s.key - base)] = SliceRegion {
                start: (start / frames as f64) as f32,
                end: (end / frames as f64) as f32,
                pitch: request.pitch,
                attack_ms: request.attack_ms,
                release_ms: request.release_ms,
                reverse: s.reverse,
                label: s.name.clone(),
                ..Default::default()
            };
        }
        asset.slices = slices;
        let command = Command::SetSamplePlayback {
            channel: request.channel,
            asset: Some(asset),
        };
        if let Err(error) = document.dispatch(command, None) {
            warnings.push(
                windfall_core::notice!("flp.slices.refused", error = error.to_string()).to_string(),
            );
            continue;
        }
        if let Some(channel) = document.project().channel(request.channel)
            && let windfall_project::ChannelSource::Instrument { mut params, .. } = channel.source
        {
            if let Some(index) = params
                .kind()
                .descriptors()
                .iter()
                .position(|p| p.id == "baseKey")
            {
                params.set(index, f32::from(base));
            }
            if let Err(error) = document.dispatch(
                Command::SetInstrumentParams {
                    channel: request.channel,
                    params,
                },
                None,
            ) {
                warnings.push(error.to_string());
            }
        }
    }
    *project = document.project().clone();
    warnings
}

/// Resolve public peak-to-0dB normalization only with decoded sample evidence.
/// This runs during review, never on the audio callback.
pub fn resolve_normalization(
    project: &mut Project,
    requests: &[(ChannelId, SampleId)],
    peaks: &[(SampleId, f32)],
) -> Vec<String> {
    let mut doc = Document::new(project.clone());
    let mut warnings = Vec::new();
    for &(id, sample) in requests {
        let Some((_, peak)) = peaks
            .iter()
            .find(|(s, p)| *s == sample && p.is_finite() && *p > 0.0)
        else {
            warnings.push(
                windfall_core::notice!(
                    "flp.slices.normalizationNeedsAudio",
                    channel = id.0,
                    sample = sample.0
                )
                .to_string(),
            );
            continue;
        };
        let Some(channel) = doc.project().channel(id) else {
            continue;
        };
        let windfall_project::ChannelSource::Sampler(settings) = &channel.source else {
            continue;
        };
        let gain = settings.gain / peak;
        if gain > windfall_project::MAX_GAIN {
            warnings.push(
                windfall_core::notice!(
                    "flp.slices.normalizationClamped",
                    channel = id.0,
                    gain = gain,
                    max = windfall_project::MAX_GAIN
                )
                .to_string(),
            );
        }
        if let Err(e) = doc.dispatch(
            Command::UpdateSampler {
                id,
                patch: windfall_project::SamplerPatch {
                    gain: Some(gain.min(windfall_project::MAX_GAIN)),
                    ..Default::default()
                },
            },
            None,
        ) {
            warnings.push(e.to_string());
        }
    }
    *project = doc.project().clone();
    warnings
}

pub fn resolve_sample_trim(
    project: &mut Project,
    requests: &[(ChannelId, SampleId, f32)],
    pcm: &[(SampleId, &[f32], usize)],
) -> Vec<String> {
    let mut doc = Document::new(project.clone());
    let mut warnings = Vec::new();
    for &(id, sample, threshold) in requests {
        let Some((_, data, channels)) = pcm.iter().find(|(s, _, c)| *s == sample && *c > 0) else {
            warnings.push(
                windfall_core::notice!(
                    "flp.slices.trimNeedsAudio",
                    channel = id.0,
                    sample = sample.0
                )
                .to_string(),
            );
            continue;
        };
        let frames = data.len() / channels;
        let audible = |f: &[f32]| f.iter().any(|v| v.is_finite() && v.abs() > threshold);
        let Some(first) = data.chunks_exact(*channels).position(audible) else {
            warnings.push(
                windfall_core::notice!("flp.slices.trimWholeSample", channel = id.0).to_string(),
            );
            continue;
        };
        let last = data.chunks_exact(*channels).rposition(audible).unwrap() + 1;
        let Some(c) = doc.project().channel(id) else {
            continue;
        };
        let windfall_project::ChannelSource::Sampler(settings) = &c.source else {
            continue;
        };
        let start = settings.start.max(first as f32 / frames as f32);
        let end = settings.end.min(last as f32 / frames as f32);
        if start >= end {
            warnings.push(
                windfall_core::notice!("flp.slices.trimOutsideWindow", channel = id.0).to_string(),
            );
            continue;
        }
        if let Err(e) = doc.dispatch(
            Command::UpdateSampler {
                id,
                patch: windfall_project::SamplerPatch {
                    start: Some(start),
                    end: Some(end),
                    ..Default::default()
                },
            },
            None,
        ) {
            warnings.push(e.to_string());
        }
    }
    *project = doc.project().clone();
    warnings
}

pub fn resolve_layer_samples(
    project: &mut Project,
    requests: &[(ChannelId, SampleId)],
    pcm: &[(SampleId, windfall_core::AudioBuffer)],
) -> Vec<String> {
    let mut doc = Document::new(project.clone());
    let mut warnings = Vec::new();
    for &(channel, sample) in requests {
        let Some((_, buffer)) = pcm.iter().find(|(s, _)| *s == sample) else {
            warnings.push(
                windfall_core::notice!(
                    "flp.slices.layerKickNeedsAudio",
                    channel = channel.0,
                    sample = sample.0
                )
                .to_string(),
            );
            continue;
        };
        let Some(source) = doc.project().channel(channel).map(|c| c.source.clone()) else {
            continue;
        };
        let mono: Vec<_> = buffer
            .samples()
            .chunks_exact(usize::from(buffer.channels()))
            .take(windfall_dsp::instrument_asset::MAX_LAYER_FRAMES + 1)
            .map(|f| f.iter().copied().sum::<f32>() / f.len() as f32)
            .collect();
        if buffer.frames() > windfall_dsp::instrument_asset::MAX_LAYER_FRAMES
            || buffer.channels() != 1
        {
            warnings.push(
                windfall_core::notice!("flp.slices.layerKickMono", sample = sample.0).to_string(),
            );
        }
        let asset = windfall_dsp::InstrumentAsset::Pcm(
            windfall_dsp::instrument_asset::LayerPcm::from_samples(buffer.sample_rate(), &mono),
        );
        if let Err(e) = doc.dispatch(
            Command::SetInstrumentAsset {
                channel,
                expected: source,
                asset: Some(asset),
            },
            None,
        ) {
            warnings.push(e.to_string());
        }
    }
    *project = doc.project().clone();
    warnings
}
