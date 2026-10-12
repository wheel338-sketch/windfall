//! Control-side clip timing mirrors clips::Slot::place and plan's source skip.
use crate::{SamplePool, SongClock};
use windfall_core::AudioBuffer;
use windfall_project::{AudioViewTiming, Clip, ClipContent};

pub fn view_timing(
    pool: &SamplePool,
    clip: &Clip,
    tempo: f64,
    clock: &SongClock,
) -> Result<AudioViewTiming, String> {
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
    let start = clock.seconds_at(f64::from(clip.start))?;
    let end_tick = clip
        .start
        .checked_add(clip.length)
        .ok_or("Invalid clip duration.")?;
    let end = clock.seconds_at(f64::from(end_tick))?;
    let timing = AudioViewTiming {
        duration_frames: (end - start) * rate,
        // Source offsets are in stored-tempo ticks, just like PlanAudioClip::skip.
        offset_frames: f64::from(clip.offset) * 60.0 * rate
            / (tempo * f64::from(windfall_core::PPQ)),
        fade_in_frames: (clock.seconds_at(f64::from(clip.start + fade_in.min(clip.length)))?
            - start)
            * rate,
        fade_out_frames: (end
            - clock.seconds_at(f64::from(end_tick - fade_out.min(clip.length)))?)
            * rate,
    };
    timing.check()?;
    Ok(timing)
}

pub fn render_view_timed(
    pool: &SamplePool,
    clip: &Clip,
    timing: &AudioViewTiming,
) -> Result<AudioBuffer, String> {
    super::render_timed(pool, clip, timing)
}
