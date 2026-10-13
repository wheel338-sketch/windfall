//! Immutable, control-compiled navigation. Callback transitions are bounded.
use windfall_project::{MarkerKind, TickRange, Timeline};

/// A callback exceeding this bound stops at its current place and reports it.
pub const MAX_NAVIGATION_TRANSITIONS: u32 = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) enum NavigationAction {
    Jump(u32),
    Pause,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct NavigationPoint {
    pub tick: u32,
    pub action: NavigationAction,
    pub looping: bool,
}

pub(crate) fn compile(timeline: &Timeline) -> Vec<NavigationPoint> {
    let mut points: Vec<_> = timeline
        .markers
        .iter()
        .filter_map(|m| match m.kind {
            MarkerKind::Named | MarkerKind::Start | MarkerKind::Punch { .. } => None,
            MarkerKind::Pause => Some(NavigationPoint {
                tick: m.tick,
                action: NavigationAction::Pause,
                looping: false,
            }),
            MarkerKind::Skip { end } => Some(NavigationPoint {
                tick: m.tick,
                action: NavigationAction::Jump(end),
                looping: false,
            }),
            MarkerKind::Loop { end } => Some(NavigationPoint {
                tick: end,
                action: NavigationAction::Jump(m.tick),
                looping: true,
            }),
        })
        .collect();
    points.sort_by_key(|p| p.tick);
    points
}

/// Endpoint frame rounding is absolute, so selecting adjacent regions cannot
/// add a frame by repeatedly rounding each duration independently.
pub(crate) fn region_frames(plan: &crate::plan::Plan, range: TickRange, rate: u32) -> (u64, u64) {
    let per_tick = windfall_core::samples_per_tick(plan.tempo_bpm, f64::from(rate));
    (
        (plan.warp(f64::from(range.start)) * per_tick).ceil() as u64,
        (plan.warp(f64::from(range.end)) * per_tick).ceil() as u64,
    )
}

/// A selected playback anchor shares the absolute sample grid of exports.
pub(crate) fn aligned_tick(plan: &crate::plan::Plan, tick: f64, rate: f64) -> f64 {
    let per_tick = windfall_core::samples_per_tick(plan.tempo_bpm, rate);
    plan.unwarp((plan.warp(tick) * per_tick - 1e-6).ceil() / per_tick)
}
