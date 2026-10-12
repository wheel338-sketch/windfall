//! Long classical dereverb capture: no whole-file bytes, decoded copy, rendered
//! view or planar source copy. Loaded project PCM is borrowed only during capture.
use super::{error, native, native_error};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use windfall_core::AudioBuffer;
use windfall_project::{AudioViewTiming, Clip, ClipContent, ClipStretch};

pub(super) const STREAM_PCM_LIMIT: u64 = 128 * 1024 * 1024;
pub(super) const SOURCE_FILE_LIMIT: u64 = 1024 * 1024 * 1024;
pub(super) fn limits() -> native::InputLimits {
    native::InputLimits {
        max_frames: 192_000 * 3600,
        max_bytes: STREAM_PCM_LIMIT,
        ..Default::default()
    }
}

pub(super) fn verify_source(
    path: &Path,
    source: &AudioBuffer,
    work: &mut native::Work,
) -> Result<(), String> {
    // DrumSynth is a bounded synthesis recipe, rather than a packet decoder.
    // Preserve its previously supported short-source path within the fixed
    // capture scratch allowance; long actual audio always uses block decoding.
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ds"))
    {
        work.check().map_err(native_error)?;
        let decoded = windfall_codec::decode_file_with(
            path,
            &windfall_codec::DecodeOptions {
                max_decoded_bytes: 16 * 1024 * 1024,
            },
        )
        .map_err(|e| error("io", e))?;
        if native::ContentFingerprint::audio(&decoded, work).map_err(native_error)?
            != native::ContentFingerprint::audio(source, work).map_err(native_error)?
        {
            return Err(error(
                "stale",
                "The source file changed; reload its audio first.",
            ));
        }
        return work.check().map_err(native_error);
    }
    let mut offset = 0_usize;
    let shape = windfall_codec::decode_file_blocks::<native::AnalysisError>(
        path,
        &windfall_codec::DecodeOptions {
            max_decoded_bytes: windfall_codec::DEFAULT_MAX_DECODED_BYTES,
        },
        |rate, channels, block| {
            work.checkpoint(block.len() as u64)?;
            if rate != source.sample_rate()
                || channels != source.channels()
                || offset
                    .checked_add(block.len())
                    .is_none_or(|end| end > source.samples().len())
                || source.samples()[offset..offset + block.len()]
                    .iter()
                    .zip(block)
                    .any(|(a, b)| a.to_bits() != b.to_bits())
            {
                return Err(native::AnalysisError::Stale);
            }
            offset += block.len();
            Ok(())
        },
    )
    .map_err(native_error)?;
    if shape
        != (
            source.sample_rate(),
            source.channels(),
            source.frames() as u64,
        )
        || offset != source.samples().len()
    {
        return Err(error(
            "stale",
            "The source file changed; reload its audio first.",
        ));
    }
    Ok(())
}

pub(super) fn capture(
    root: &Path,
    source: &AudioBuffer,
    clip: &Clip,
    timing: &AudioViewTiming,
    stamp: native::CaptureStamp,
    work: &mut native::Work,
) -> Result<native::CapturedInput, String> {
    // Worst DSP construction peak is 32 MiB plus bounded file/IO chunks.
    // Input/output files are independently preflighted by the caller.
    let mut prepared = None;
    let mut frames = source.frames();
    if !matches!(source.channels(), 1 | 2) || !(8000..=192000).contains(&source.sample_rate()) {
        return Err(error(
            "invalid",
            "Dereverb requires mono/stereo at 8â€“192 kHz.",
        ));
    }
    if let ClipContent::Audio {
        stretch:
            ClipStretch::Spectral {
                ratio,
                quality,
                formants,
            },
        pitch,
        ..
    } = clip.content
    {
        frames = windfall_stretch::stretched_frames(source.frames(), ratio);
        let bytes = (frames as u64)
            .checked_mul(u64::from(source.channels()) * 4)
            .ok_or_else(|| error("budget", "Spectral file shape overflow."))?;
        if bytes > STREAM_PCM_LIMIT {
            return Err(error(
                "budget",
                "Spectral preparation file limit is 128 MiB.",
            ));
        }
        let mut file = tempfile::tempfile_in(root).map_err(|e| error("io", e))?;
        let mut failure = None::<String>;
        let mut written = 0_u64;
        let done = windfall_stretch::stretch_blocks_cancellable(
            source,
            ratio,
            f64::from(pitch),
            match quality {
                windfall_project::ClipStretchQuality::Fast => windfall_stretch::Quality::Fast,
                windfall_project::ClipStretchQuality::Standard => {
                    windfall_stretch::Quality::Standard
                }
                windfall_project::ClipStretchQuality::High => windfall_stretch::Quality::High,
            },
            formants,
            &mut |samples| {
                let checked = if samples.is_empty() {
                    work.checkpoint(4096 * 64)
                } else {
                    work.check()
                };
                if let Err(e) = checked {
                    failure = Some(native_error(e));
                    return false;
                }
                written += samples.len() as u64 * 4;
                if written > bytes {
                    failure = Some(error("budget", "Spectral file length overflow."));
                    return false;
                }
                let mut encoded = [0_u8; 16384];
                for (target, sample) in encoded.chunks_exact_mut(4).zip(samples) {
                    if !sample.is_finite() {
                        failure = Some(error("invalid", "Nonfinite spectral preparation."));
                        return false;
                    }
                    target.copy_from_slice(&sample.to_le_bytes());
                }
                if let Err(e) = file.write_all(&encoded[..samples.len() * 4]) {
                    failure = Some(error("io", e));
                    return false;
                }
                true
            },
        );
        if let Some(error) = failure {
            return Err(error);
        }
        if !done || written != bytes {
            return Err(error("cancelled", "Spectral preparation did not finish."));
        }
        file.flush().map_err(|e| error("io", e))?;
        prepared = Some(file);
    }
    // One whole-frame file page, also used for reverse and fractional pitch.
    let channels = usize::from(source.channels());
    let mut page = [0_u8; 16384];
    let mut page_start = usize::MAX;
    native::CapturedInput::capture_stream(root, stamp, limits(), work, |work, emit| {
        let result = windfall_engine::audio_edit::stream_prepared_view(
            clip,
            timing,
            frames,
            |frame| {
                let Some(file) = &mut prepared else {
                    return Ok(source.stereo_frame(frame));
                };
                let first = frame / (4096 / channels) * (4096 / channels);
                if first != page_start {
                    file.seek(SeekFrom::Start((first * channels * 4) as u64))
                        .map_err(|e| error("io", e))?;
                    let count = (frames - first).min(4096 / channels) * channels * 4;
                    file.read_exact(&mut page[..count])
                        .map_err(|e| error("io", e))?;
                    page_start = first;
                }
                let offset = (frame - first) * channels * 4;
                let a =
                    f32::from_le_bytes(page[offset..offset + 4].try_into().expect("four bytes"));
                let b = if channels == 1 {
                    a
                } else {
                    f32::from_le_bytes(page[offset + 4..offset + 8].try_into().expect("four bytes"))
                };
                Ok((a, b))
            },
            |samples| emit(samples, work).map_err(native_error),
        );
        work.check()?;
        result.map_err(native::AnalysisError::Adapter)
    })
    .map_err(native_error)
}
