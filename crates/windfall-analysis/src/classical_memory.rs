//! Conservative aggregate preflight for the classical editor kernels. These
//! are worker heap bounds, including returned PCM/estimates, not per-buffer
//! limits. Runtime allowances include FFT plan construction and vector growth.
use crate::{AnalysisError, Result, add};
use windfall_core::AudioBuffer;
use windfall_project::AudioEditRecipe;

pub const CLASSICAL_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
const FFT_RUNTIME: u64 = 8 * 1024 * 1024;
const STRETCH_RUNTIME: u64 = 32 * 1024 * 1024;
const METADATA: u64 = 2 * 1024 * 1024;

fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b).ok_or(AnalysisError::Exhausted)
}

/// Additional memory above the borrowed input. Includes the YIN estimate and
/// segment arrays at every 10 ms cell, sparse FFT candidates, note/count vector
/// growth, transients, and returned estimates; detection maths stay unchanged.
pub fn detection(audio: &AudioBuffer) -> Result<u64> {
    let cells = (audio.frames() as u64).div_ceil(u64::from((audio.sample_rate() / 100).max(1)));
    add(add(FFT_RUNTIME, METADATA)?, mul(cells, 256)?)
}

/// f64 overlap-add accumulator + f64 frame weights + returned f32 PCM coexist.
pub fn denoising(audio: &AudioBuffer) -> Result<u64> {
    add(
        FFT_RUNTIME,
        add(
            mul(audio.samples().len() as u64, 12)?,
            mul(audio.frames() as u64, 8)?,
        )?,
    )
}

/// Covers both denoise preparation and the largest contextual stretch interval:
/// retained denoised source, full final output, interleaved interval, planar
/// copy (including growth), interval result, previous/new seam tails and all
/// stretcher/FFT construction storage. Refuses before allocating any of these.
pub fn rendering(audio: &AudioBuffer, recipe: &AudioEditRecipe) -> Result<u64> {
    recipe.check().map_err(AnalysisError::Adapter)?;
    if audio.frames() != recipe.analysis.frames as usize
        || audio.sample_rate() != recipe.analysis.sample_rate
    {
        return Err(AnalysisError::Stale);
    }
    let channels = u64::from(audio.channels());
    let metadata = add(METADATA, recipe_bytes(recipe)?)?;
    let output = mul(
        mul(
            u64::from(recipe.output_frames().map_err(AnalysisError::Adapter)?),
            channels,
        )?,
        4,
    )?;
    let points = recipe.render_markers().map_err(AnalysisError::Adapter)?;
    let denoises = recipe
        .denoise
        .is_some_and(|settings| settings.wet != 0.0 && settings.reduction_db != 0.0);
    if points
        .iter()
        .all(|marker| marker.source_frame == marker.target_frame)
        && recipe
            .overrides
            .iter()
            .all(|edit| edit.midi_pitch == recipe.analysis.notes[edit.note as usize].midi_pitch)
    {
        return if denoises {
            add(denoising(audio)?, metadata)
        } else {
            Ok(metadata)
        };
    }
    let context = u64::from(audio.sample_rate()) * 120 / 1000;
    let mut interval_peak = 0;
    for pair in points.windows(2) {
        let first = u64::from(pair[0].source_frame).saturating_sub(context);
        let last = (u64::from(pair[1].source_frame) + context).min(audio.frames() as u64);
        let input = mul(mul(last - first, channels)?, 4)?;
        let ratio = f64::from(pair[1].target_frame - pair[0].target_frame)
            / f64::from(pair[1].source_frame - pair[0].source_frame);
        let transformed = mul(
            mul(((last - first) as f64 * ratio).round() as u64, channels)?,
            4,
        )?;
        interval_peak = interval_peak.max(add(mul(input, 3)?, transformed)?);
    }
    let source = if denoises {
        mul(audio.samples().len() as u64, 4)?
    } else {
        0
    };
    let render = add(
        add(add(source, output)?, interval_peak)?,
        add(STRETCH_RUNTIME, metadata)?,
    )?;
    Ok(if denoises {
        render.max(add(denoising(audio)?, metadata)?)
    } else {
        render
    })
}

/// Count caller-supplied capacities, not just valid lengths. Multiple recipes
/// (old cache, incoming edit, retained proposal and IPC response) coexist.
pub fn recipe_bytes(recipe: &AudioEditRecipe) -> Result<u64> {
    let mut bytes = std::mem::size_of::<AudioEditRecipe>() as u64;
    bytes = add(
        bytes,
        mul(
            recipe.analysis.notes.capacity() as u64,
            std::mem::size_of::<windfall_project::DetectedAudioNote>() as u64,
        )?,
    )?;
    bytes = add(bytes, mul(recipe.analysis.transients.capacity() as u64, 4)?)?;
    bytes = add(
        bytes,
        mul(
            recipe.markers.capacity() as u64,
            std::mem::size_of::<windfall_project::AudioWarpMarker>() as u64,
        )?,
    )?;
    bytes = add(
        bytes,
        mul(
            recipe.overrides.capacity() as u64,
            std::mem::size_of::<windfall_project::AudioNoteOverride>() as u64,
        )?,
    )?;
    Ok(bytes)
}
