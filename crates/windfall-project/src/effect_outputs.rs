//! Routing of the separately routable outputs some effects have.
//!
//! Most effects have one output: the next place in their track's chain. A
//! few also have outputs of their own. Send Tap has a copy of the signal at
//! its place in the chain, and Transient Split has its transient and
//! sustained parts. A route sends one such output to another mixer track,
//! where it is summed with everything else that arrives there.
//!
//! Routes are stored by effect id, beside the tracks, so an effect keeps
//! its routes when it moves along its chain and loses them when it is
//! deleted. A route leaves its track before the fader, at the effect's
//! place in the chain.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{EffectId, EffectKind, Mixer, MixerTrack, Project, TrackId};

/// One output of an effect, sent to another mixer track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EffectOutputRoute {
    pub effect: EffectId,
    /// Which of the effect's routable outputs, counted from 0 in the order
    /// the effect lists them.
    pub output: u8,
    /// The track that receives it. Never the effect's own track.
    pub target: TrackId,
}

/// Number of routable outputs an effect of this kind has.
pub fn output_count(kind: EffectKind) -> usize {
    kind.aux_outputs().len()
}

/// The track an effect sits on.
pub(crate) fn home(mixer: &Mixer, effect: EffectId) -> Option<&MixerTrack> {
    mixer
        .tracks
        .iter()
        .find(|track| track.effects.iter().any(|slot| slot.id == effect))
}

/// The tracks the effects of `from` send their routed outputs to.
pub(crate) fn targets(mixer: &Mixer, from: TrackId) -> impl Iterator<Item = TrackId> + '_ {
    let track = mixer.track(from);
    mixer
        .effect_outputs
        .iter()
        .filter(move |route| {
            track.is_some_and(|track| track.effects.iter().any(|slot| slot.id == route.effect))
        })
        .map(|route| route.target)
}

/// Why `route` cannot be part of `mixer` as it stands, leaving loops aside.
fn problem(mixer: &Mixer, route: &EffectOutputRoute) -> Option<String> {
    let name = format!(
        "output {} of effect {}",
        u32::from(route.output) + 1,
        route.effect.0
    );
    let Some(track) = home(mixer, route.effect) else {
        return Some(format!("{name} is routed, but the effect does not exist"));
    };
    let Some(slot) = track.effects.iter().find(|slot| slot.id == route.effect) else {
        return Some(format!("{name} is routed, but the effect does not exist"));
    };
    if usize::from(route.output) >= output_count(slot.kind()) {
        return Some(format!("{name} is routed, but the effect has no such output"));
    }
    if track.id == TrackId::MASTER || track.current {
        return Some(format!(
            "{name} is routed from a track that cannot feed other tracks"
        ));
    }
    let Some(target) = mixer.track(route.target) else {
        return Some(format!(
            "{name} is routed to mixer track {}, which does not exist",
            route.target.0
        ));
    };
    if target.id == track.id {
        return Some(format!("{name} is routed back into its own track"));
    }
    if target.current {
        return Some("Current cannot receive ordinary mixer routes".to_owned());
    }
    None
}

/// Checks every route of the project. Routing loops are checked with the
/// rest of the mixer's routing.
pub(crate) fn check(project: &Project) -> Result<(), String> {
    let mixer = &project.mixer;
    let mut seen = std::collections::HashSet::new();
    for route in &mixer.effect_outputs {
        if let Some(problem) = problem(mixer, route) {
            return Err(problem);
        }
        if !seen.insert((route.effect, route.output)) {
            return Err(format!(
                "output {} of effect {} is routed more than once",
                u32::from(route.output) + 1,
                route.effect.0
            ));
        }
    }
    Ok(())
}

/// The routes that still make sense after an edit to the tracks: an effect
/// that was deleted or replaced by a kind without the output loses its
/// routes, as does one whose target is gone or is now its own track, and a
/// route that would close a routing loop where it now sits is dropped
/// rather than kept broken. Order is preserved.
pub(crate) fn pruned(mixer: &Mixer) -> Vec<EffectOutputRoute> {
    if mixer.effect_outputs.is_empty() {
        return Vec::new();
    }
    let mut kept = mixer.clone();
    let mut seen = std::collections::HashSet::new();
    kept.effect_outputs
        .retain(|route| problem(mixer, route).is_none() && seen.insert((route.effect, route.output)));
    // Each route is tried against the routing that survived before it.
    let mut index = 0;
    while index < kept.effect_outputs.len() {
        let route = kept.effect_outputs[index];
        let source = home(&kept, route.effect).map(|track| track.id);
        kept.effect_outputs.remove(index);
        let loops = source.is_none_or(|source| crate::check::reaches(&kept, route.target, source));
        if !loops {
            kept.effect_outputs.insert(index, route);
            index += 1;
        }
    }
    kept.effect_outputs
}

/// The routes with `output` of `effect` sent to `target`, or disconnected
/// when `target` is `None`. The caller checks the effect and the tracks.
pub(crate) fn with_route(
    routes: &[EffectOutputRoute],
    effect: EffectId,
    output: u8,
    target: Option<TrackId>,
) -> Vec<EffectOutputRoute> {
    let mut routes = routes.to_vec();
    let existing = routes
        .iter()
        .position(|route| route.effect == effect && route.output == output);
    match (target, existing) {
        (Some(target), Some(index)) => routes[index].target = target,
        (Some(target), None) => routes.push(EffectOutputRoute {
            effect,
            output,
            target,
        }),
        (None, Some(index)) => {
            routes.remove(index);
        }
        (None, None) => {}
    }
    routes
}
