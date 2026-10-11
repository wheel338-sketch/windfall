//! Layer channels: a channel that plays its notes on other channels.
//!
//! The settings live in [`crate::LayerSettings`] on the channel's voice
//! tools. This module holds the rules that need the whole rack: every child
//! exists, a layer never plays itself, and layers never form a loop. It also
//! resolves a layer to the channels that finally sound, which is what the
//! engine and note export follow.

use crate::{Channel, ChannelId, ChannelVoiceSettings, MAX_LAYER_CHILDREN};

/// The most channels one note can reach through nested layers. Past this the
/// remaining channels are left out, so a note is always a bounded amount of
/// work on the audio thread.
pub const MAX_LAYER_TARGETS: usize = 64;

fn find(channels: &[Channel], id: ChannelId) -> Option<usize> {
    channels.iter().position(|channel| channel.id == id)
}

/// Whether `settings` may be given to channel `id` of this rack.
///
/// The children are checked whether or not the layer is switched on, so
/// switching it on later cannot make a loop.
pub fn check_settings(
    channels: &[Channel],
    id: ChannelId,
    settings: &ChannelVoiceSettings,
) -> Result<(), String> {
    let layer = settings.layer;
    if layer.children.len() > MAX_LAYER_CHILDREN {
        return Err(format!(
            "a layer plays at most {MAX_LAYER_CHILDREN} channels"
        ));
    }
    for child in layer.children.iter() {
        if child == id {
            return Err("a layer cannot play itself".into());
        }
        if find(channels, child).is_none() {
            return Err(format!(
                "layer child channel {} is not in the project",
                child.0
            ));
        }
    }
    // Walk down from the children. Reaching `id` again is a loop. The walk
    // reads the other channels' saved children and this channel's new ones.
    let mut seen = vec![false; channels.len()];
    let mut pending: Vec<ChannelId> = layer.children.iter().collect();
    while let Some(next) = pending.pop() {
        if next == id {
            return Err("layers cannot play each other in a loop".into());
        }
        let Some(index) = find(channels, next) else {
            continue;
        };
        if std::mem::replace(&mut seen[index], true) {
            continue;
        }
        pending.extend(channels[index].voice.layer.children.iter());
    }
    Ok(())
}

/// Checks every channel of the rack. For the whole-project check.
pub fn check(channels: &[Channel]) -> Result<(), String> {
    for channel in channels {
        check_settings(channels, channel.id, &channel.voice)
            .map_err(|error| format!("channel {}: {error}", channel.id.0))?;
    }
    Ok(())
}

/// The channels that lose `removed` as a child, each with its new settings.
/// Used when a channel is deleted, so no layer is left pointing at it.
pub fn without_child(channels: &[Channel], removed: ChannelId) -> Vec<(usize, Channel)> {
    channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| channel.id != removed)
        .filter_map(|(index, channel)| {
            let mut next = channel.clone();
            next.voice
                .layer
                .children
                .remove(removed)
                .then_some((index, next))
        })
        .collect()
}

/// Indexes of the channels that sound when channel `index` is given a note:
/// the channel itself when it is not a layer, and otherwise its children,
/// with nested layers followed down to channels that make sound. Each
/// channel appears once, in rack-walk order. Layers themselves are never in
/// the list, so a layer with no children is silent. Loops, which the checks
/// refuse, end the walk instead of repeating.
pub fn targets(channels: &[Channel], index: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let Some(channel) = channels.get(index) else {
        return out;
    };
    if !channel.voice.layer.enabled {
        out.push(index);
        return out;
    }
    let mut visited = vec![false; channels.len()];
    visited[index] = true;
    // Depth first, children in their saved order.
    let mut pending: Vec<ChannelId> = channel.voice.layer.children.iter().collect();
    pending.reverse();
    while let Some(next) = pending.pop() {
        let Some(child) = find(channels, next) else {
            continue;
        };
        if std::mem::replace(&mut visited[child], true) {
            continue;
        }
        let layer = channels[child].voice.layer;
        if layer.enabled {
            let mut nested: Vec<ChannelId> = layer.children.iter().collect();
            nested.reverse();
            pending.extend(nested);
        } else if out.len() < MAX_LAYER_TARGETS {
            out.push(child);
        }
    }
    out
}

/// Whether the channel is a layer that is switched on.
pub fn is_layer(channel: &Channel) -> bool {
    channel.voice.layer.enabled
}

/// Which channels solo leaves audible, by index. A soloed layer keeps the
/// channels it plays audible, because it has no sound of its own to solo.
/// Everything is audible when no channel is soloed.
pub fn solo_audible(channels: &[Channel]) -> Vec<bool> {
    if !channels.iter().any(|channel| channel.solo) {
        return vec![true; channels.len()];
    }
    let mut audible: Vec<bool> = channels.iter().map(|channel| channel.solo).collect();
    for (index, channel) in channels.iter().enumerate() {
        if channel.solo && is_layer(channel) {
            for target in targets(channels, index) {
                audible[target] = true;
            }
        }
    }
    audible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChannelSource, ChannelTiming, LayerChildren, LayerSettings, SamplerSettings, TrackId,
    };

    fn channel(id: u32, layer: Option<&[u32]>) -> Channel {
        Channel {
                sample_playback: None,
            voice: ChannelVoiceSettings {
                layer: LayerSettings {
                    enabled: layer.is_some(),
                    children: LayerChildren::new(
                        layer.unwrap_or(&[]).iter().map(|&id| ChannelId(id)),
                    ),
                },
                ..Default::default()
            },
            id: ChannelId(id),
            name: String::new(),
            group: String::new(),
            timing: ChannelTiming::default(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            mixer_track: TrackId::MASTER,
            source: ChannelSource::Sampler(SamplerSettings::default()),
            expression_map: Default::default(),
        }
    }

    #[test]
    fn children_keep_order_drop_repeats_and_stop_at_the_bound() {
        let children = LayerChildren::new([3, 1, 3, 2].map(ChannelId));
        assert_eq!(
            children.iter().map(|id| id.0).collect::<Vec<_>>(),
            [3, 1, 2]
        );
        let full = LayerChildren::new((0..40).map(ChannelId));
        assert_eq!(full.len(), MAX_LAYER_CHILDREN);
        let mut edited = children;
        assert!(edited.remove(ChannelId(1)));
        assert!(!edited.remove(ChannelId(1)));
        assert_eq!(edited.iter().map(|id| id.0).collect::<Vec<_>>(), [3, 2]);
        let json = serde_json::to_string(&edited).unwrap();
        assert_eq!(json, "[3,2]");
        assert_eq!(
            serde_json::from_str::<LayerChildren>(&json).unwrap(),
            edited
        );
        let long = serde_json::to_string(&(0..17).collect::<Vec<u32>>()).unwrap();
        assert!(serde_json::from_str::<LayerChildren>(&long).is_err());
    }

    #[test]
    fn a_plain_channel_plays_itself_and_a_layer_plays_its_leaves_once() {
        let rack = [
            channel(1, Some(&[2, 3, 4])),
            channel(2, None),
            channel(3, Some(&[4, 5])),
            channel(4, None),
            channel(5, None),
        ];
        assert_eq!(targets(&rack, 1), [1]);
        // Channel 4 is reached twice and plays once; layer 3 is not a target.
        assert_eq!(targets(&rack, 0), [1, 3, 4]);
        assert_eq!(targets(&rack, 2), [3, 4]);
        assert!(targets(&[channel(1, Some(&[]))], 0).is_empty());
        assert!(targets(&rack, 9).is_empty());
    }

    #[test]
    fn self_missing_and_looping_children_are_refused() {
        let rack = [
            channel(1, Some(&[2])),
            channel(2, Some(&[3])),
            channel(3, None),
        ];
        assert!(check(&rack).is_ok());
        let own = channel(1, Some(&[1])).voice;
        assert!(check_settings(&rack, ChannelId(1), &own).is_err());
        let missing = channel(1, Some(&[9])).voice;
        assert!(check_settings(&rack, ChannelId(1), &missing).is_err());
        // 3 -> 1 would close the loop 1 -> 2 -> 3 -> 1.
        let looping = channel(3, Some(&[1])).voice;
        assert!(check_settings(&rack, ChannelId(3), &looping).is_err());
        // A loop left in a damaged file does not hang the resolver.
        let damaged = [channel(1, Some(&[2])), channel(2, Some(&[1]))];
        assert!(check(&damaged).is_err());
        assert!(targets(&damaged, 0).is_empty());
    }

    #[test]
    fn removing_a_child_edits_only_the_layers_that_held_it() {
        let rack = [
            channel(1, Some(&[2, 3])),
            channel(2, None),
            channel(3, None),
        ];
        let changed = without_child(&rack, ChannelId(3));
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, 0);
        assert_eq!(
            changed[0].1.voice.layer.children.iter().collect::<Vec<_>>(),
            [ChannelId(2)]
        );
        assert!(without_child(&rack, ChannelId(1)).is_empty());
    }

    #[test]
    fn a_soloed_layer_keeps_its_children_audible() {
        let mut rack = [channel(1, Some(&[2])), channel(2, None), channel(3, None)];
        assert_eq!(solo_audible(&rack), [true, true, true]);
        rack[0].solo = true;
        assert_eq!(solo_audible(&rack), [true, true, false]);
        rack[0].solo = false;
        rack[2].solo = true;
        assert_eq!(solo_audible(&rack), [false, false, true]);
    }
}
