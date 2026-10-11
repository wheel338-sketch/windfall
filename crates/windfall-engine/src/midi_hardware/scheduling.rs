//! Audio/MIDI output share the monotonic DAC clock, not worker wake-up time.
use crate::{Playhead, recording_clock::OutputClock};

pub(super) fn deadline(clock: Option<OutputClock>, frame: u64, now: u64, latency: u64) -> u64 {
    let audio = clock.map_or(now, |clock| clock.nanos_at(frame));
    // A vanished stream's stale clock cannot keep messages pending indefinitely.
    let audio = if audio > now.saturating_add(2_000_000_000) {
        now
    } else {
        audio
    };
    audio.saturating_sub(latency)
}

pub(super) fn aligned_head(
    head: &Playhead,
    frame: u64,
    clock: Option<OutputClock>,
    now: u64,
    latency: u64,
) -> Playhead {
    let Some(clock) = clock else {
        return *head;
    };
    let at = clock.nanos_at(frame);
    let hear = now.saturating_add(latency);
    let delta = if hear >= at {
        (hear - at) as f64
    } else {
        -((at - hear) as f64)
    };
    let mut head = *head;
    head.tick += delta * 1e-9 * head.ticks_per_second;
    if let Some(length) = head
        .pattern_ticks
        .filter(|length| *length > 0 && head.mode == windfall_ipc::PlayMode::Pattern)
    {
        head.tick = head.tick.rem_euclid(f64::from(length));
    } else {
        head.tick = head.tick.max(0.0);
    }
    head
}
