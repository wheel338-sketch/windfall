//! Portable AUv2 registry, scope/bus mapping and owner lifecycle contracts.
//! No macOS frameworks are referenced here; these tests run on Windows.
use crate::{PluginDescriptor, PluginFormat, PluginKind};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentId {
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
}
impl ComponentId {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut words = text.split(':');
        let mut next = || {
            let word = words.next().ok_or("AU id requires three words")?;
            if word.len() != 8 || !word.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("AU id words must be eight hexadecimal digits".to_owned());
            }
            u32::from_str_radix(word, 16).map_err(|e| e.to_string())
        };
        let id = Self {
            kind: next()?,
            subtype: next()?,
            manufacturer: next()?,
        };
        if words.next().is_some()
            || ![
                u32::from_be_bytes(*b"aufx"),
                u32::from_be_bytes(*b"aumu"),
                u32::from_be_bytes(*b"aumf"),
                u32::from_be_bytes(*b"augn"),
            ]
            .contains(&id.kind)
        {
            return Err("Unsupported AUv2 component identity".into());
        }
        Ok(id)
    }
    pub fn id(self) -> String {
        format!(
            "{:08X}:{:08X}:{:08X}",
            self.kind, self.subtype, self.manufacturer
        )
    }
    pub fn path(self) -> String {
        format!("au://{}", self.id())
    }
    pub fn kind(self) -> PluginKind {
        if self.kind == u32::from_be_bytes(*b"aumu") || self.kind == u32::from_be_bytes(*b"augn") {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub component: ComponentId,
    pub name: String,
    pub version: u32,
}
impl CatalogEntry {
    pub fn descriptor(&self) -> PluginDescriptor {
        let (vendor, name) = self
            .name
            .split_once(':')
            .map_or(("", self.name.as_str()), |(vendor, name)| {
                (vendor.trim(), name.trim())
            });
        PluginDescriptor {
            format: PluginFormat::Au,
            id: self.component.id(),
            name: name.into(),
            vendor: vendor.into(),
            version: format!(
                "{}.{}.{}",
                self.version >> 16,
                (self.version >> 8) & 255,
                self.version & 255
            ),
            features: Vec::new(),
            kind: self.component.kind(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
}
impl Catalog {
    pub fn replace(&mut self, mut installed: Vec<CatalogEntry>) -> Result<(), String> {
        if installed.len() > 4096 {
            return Err("AU catalog exceeds 4096 components".into());
        }
        installed.sort_by_key(|entry| entry.component);
        if installed
            .windows(2)
            .any(|p| p[0].component == p[1].component)
        {
            return Err("Duplicate installed AU identity".into());
        }
        for entry in &installed {
            ComponentId::parse(&entry.component.id())?;
        }
        self.entries = installed;
        Ok(())
    }
    pub fn contains(&self, id: &str, path: &str) -> bool {
        ComponentId::parse(id).is_ok_and(|component| {
            component.path() == path
                && self
                    .entries
                    .iter()
                    .any(|entry| entry.component == component)
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterAddress {
    pub id: u32,
    pub scope: u32,
    pub element: u32,
}
/// Published id is stable AU id for global parameters (legacy bindings); scoped
/// parameters are assigned unused ids after collecting global ids, deterministically.
pub fn parameter_map(
    addresses: &[ParameterAddress],
) -> Result<Vec<(u32, ParameterAddress)>, String> {
    if addresses.len() > crate::MAX_PARAMETERS as usize {
        return Err("AU parameter bound".into());
    }
    let mut used = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    for a in addresses {
        if a.scope > 2 || !seen.insert((a.scope, a.element, a.id)) {
            return Err("Duplicate or unsupported AU parameter address".into());
        }
        if a.scope == 0 && !used.insert(a.id) {
            return Err("Duplicate global AU parameter id".into());
        }
    }
    let mut next = u32::MAX;
    let mut scoped = std::collections::BTreeMap::new();
    // Allocate synthetic IDs in canonical address order, while leaving the
    // descriptor's original presentation order untouched.
    for (scope, element, native) in seen.into_iter().filter(|a| a.0 != 0) {
        while used.contains(&next) {
            next = next
                .checked_sub(1)
                .ok_or("AU parameter id space exhausted")?;
        }
        scoped.insert((scope, element, native), next);
        used.insert(next);
    }
    Ok(addresses
        .iter()
        .map(|address| {
            let id = if address.scope == 0 {
                address.id
            } else {
                scoped[&(address.scope, address.element, address.id)]
            };
            (id, *address)
        })
        .collect())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bus {
    pub index: u32,
    pub channels: u32,
}
pub fn validate_buses(inputs: &[Bus], outputs: &[Bus]) -> Result<(), String> {
    if inputs.len() > crate::MAX_PORTS as usize
        || outputs.is_empty()
        || outputs.len() > crate::MAX_PORTS as usize
    {
        return Err("AU bus count bound".into());
    }
    for buses in [inputs, outputs] {
        for (index, bus) in buses.iter().enumerate() {
            if bus.index != index as u32
                || bus.channels == 0
                || bus.channels > crate::MAX_PORT_CHANNELS
            {
                return Err("Invalid AU bus index/channel count".into());
            }
        }
    }
    Ok(())
}
/// Fill one owned bus: mono sums stereo, wider buses keep their extra channels
/// silent. Control-side allocation makes this routine allocation-free.
pub fn copy_stereo(channels: &mut [Box<[f32]>], left: &[f32], right: &[f32]) -> bool {
    if channels.is_empty()
        || left.len() != right.len()
        || channels.iter().any(|c| c.len() < left.len())
    {
        return false;
    }
    for channel in channels.iter_mut() {
        channel.fill(0.0);
    }
    if channels.len() == 1 {
        for ((out, left), right) in channels[0].iter_mut().zip(left).zip(right) {
            *out = (*left + *right) * 0.5;
        }
    } else {
        channels[0][..left.len()].copy_from_slice(left);
        channels[1][..right.len()].copy_from_slice(right);
    }
    true
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostClock {
    pub beats: f64,
    pub tempo: f64,
    pub numerator: f32,
    pub denominator: u32,
    pub next_beat_frames: u32,
    pub downbeat: f64,
    pub timeline_sample: f64,
}
/// The AU musical-location callback uses meter beats; beat/tempo remains in
/// quarter notes. Conversion is bounded and pure for native callback use.
pub fn host_clock(clock: &crate::Transport, rate: f64) -> Option<HostClock> {
    if !rate.is_finite()
        || rate <= 0.0
        || !clock.tempo_bpm.is_finite()
        || clock.tempo_bpm <= 0.0
        || !clock.position_beats.is_finite()
        || !clock.position_seconds.is_finite()
        || clock.numerator == 0
        || clock.denominator == 0
        || !clock.denominator.is_power_of_two()
    {
        return None;
    }
    let width = 4.0 / f64::from(clock.denominator);
    let remainder = clock.position_beats.rem_euclid(width);
    let frames = if remainder == 0.0 {
        0.0
    } else {
        (width - remainder) * 60.0 * rate / clock.tempo_bpm
    };
    let timeline_sample = clock.position_seconds * rate;
    if !timeline_sample.is_finite() || !frames.is_finite() {
        return None;
    }
    Some(HostClock {
        beats: clock.position_beats,
        tempo: clock.tempo_bpm,
        numerator: f32::from(clock.numerator),
        denominator: u32::from(clock.denominator),
        next_beat_frames: frames.ceil().clamp(0.0, f64::from(u32::MAX)) as u32,
        downbeat: clock.bar_position()?.0,
        timeline_sample,
    })
}
pub fn transport_changed(
    previous: &crate::Transport,
    next: &crate::Transport,
    frames: u32,
    rate: f64,
) -> bool {
    if !rate.is_finite() || rate <= 0.0 {
        return true;
    }
    let elapsed = if previous.playing {
        f64::from(frames) / rate
    } else {
        0.0
    };
    previous.playing != next.playing
        || (next.position_seconds - previous.position_seconds - elapsed).abs() > 1.0 / rate
        || (next.position_beats - previous.position_beats - elapsed * previous.tempo_bpm / 60.0)
            .abs()
            > previous.tempo_bpm / (60.0 * rate)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateContainer {
    pub component: ComponentId,
    pub property_list: Vec<u8>,
}
impl StateContainer {
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        if self.property_list.len() > crate::MAX_STATE_BYTES - 20 {
            return Err("AU state bound".into());
        }
        let mut bytes = b"WAU1".to_vec();
        for value in [
            self.component.kind,
            self.component.subtype,
            self.component.manufacturer,
            self.property_list.len() as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.property_list);
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8], expected: ComponentId) -> Result<Self, String> {
        if bytes.len() < 20 || bytes.len() > crate::MAX_STATE_BYTES || &bytes[..4] != b"WAU1" {
            return Err("Invalid AU state container".into());
        }
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let component = ComponentId {
            kind: word(4),
            subtype: word(8),
            manufacturer: word(12),
        };
        if component != expected || word(16) as usize != bytes.len() - 20 {
            return Err("AU state identity/length mismatch".into());
        }
        Ok(Self {
            component,
            property_list: bytes[20..].to_vec(),
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderPhase {
    #[default]
    Inactive,
    Active,
    ResetPending,
    Returned,
    Failed,
}
impl ProviderPhase {
    pub fn activate(&mut self) -> Result<(), String> {
        if *self != Self::Inactive && *self != Self::Returned {
            return Err("AU owner has not proved quiescence".into());
        }
        *self = Self::Active;
        Ok(())
    }
    pub fn request_reset(&mut self) {
        if *self == Self::Active {
            *self = Self::ResetPending;
        }
    }
    pub fn render_allowed(self) -> bool {
        self == Self::Active
    }
    pub fn returned(&mut self) -> Result<(), String> {
        if !matches!(self, Self::Active | Self::ResetPending) {
            return Err("AU audio owner return mismatch".into());
        }
        *self = Self::Returned;
        Ok(())
    }
    pub fn recover(&mut self, reset_succeeded: bool) {
        if *self == Self::Returned {
            *self = if reset_succeeded {
                Self::Inactive
            } else {
                Self::Failed
            };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn component() -> ComponentId {
        ComponentId::parse("61756678:64656C79:6170706C").unwrap()
    }
    #[test]
    fn registry_identity_and_catalog_replacement_are_checked_and_deterministic() {
        let id = component();
        assert_eq!(id.path(), "au://61756678:64656C79:6170706C");
        for invalid in [
            "aufx:dely:appl",
            "61756678:64656C79:6170706C:00000000",
            "61756678:64656C79:6170706",
            "00000000:64656C79:6170706C",
        ] {
            assert!(ComponentId::parse(invalid).is_err());
        }
        let entry = CatalogEntry {
            component: id,
            name: "Apple: Delay".into(),
            version: 0x010203,
        };
        let mut catalog = Catalog::default();
        catalog.replace(vec![entry.clone()]).unwrap();
        assert!(catalog.contains(&id.id(), &id.path()));
        assert!(!catalog.contains(&id.id(), "evil.component"));
        assert!(catalog.replace(vec![entry.clone(), entry]).is_err());
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].descriptor().version, "1.2.3");
    }
    #[test]
    fn scoped_parameters_keep_global_ids_and_do_not_alias_buses() {
        let addresses = [
            ParameterAddress {
                id: 7,
                scope: 0,
                element: 0,
            },
            ParameterAddress {
                id: 7,
                scope: 1,
                element: 0,
            },
            ParameterAddress {
                id: 7,
                scope: 1,
                element: 1,
            },
        ];
        let mapped = parameter_map(&addresses).unwrap();
        assert_eq!(mapped[0].0, 7);
        assert_ne!(mapped[1].0, mapped[2].0);
        assert_eq!(mapped, parameter_map(&addresses).unwrap());
        assert!(parameter_map(&[addresses[0], addresses[0]]).is_err());
        let reordered = parameter_map(&[addresses[0], addresses[2], addresses[1]]).unwrap();
        assert_eq!(mapped[1].0, reordered[2].0);
        assert_eq!(mapped[2].0, reordered[1].0);
    }
    #[test]
    fn mono_stereo_and_sidechain_channels_have_independent_owned_storage() {
        validate_buses(
            &[
                Bus {
                    index: 0,
                    channels: 2,
                },
                Bus {
                    index: 1,
                    channels: 1,
                },
            ],
            &[Bus {
                index: 0,
                channels: 4,
            }],
        )
        .unwrap();
        let mut main = vec![vec![0.0; 4].into_boxed_slice(); 4];
        let mut key = vec![vec![0.0; 4].into_boxed_slice()];
        assert!(copy_stereo(&mut main, &[0.25; 4], &[-0.5; 4]));
        assert!(copy_stereo(&mut key, &[0.5; 4], &[0.25; 4]));
        assert_eq!(&*main[0], &[0.25; 4]);
        assert_eq!(&*main[1], &[-0.5; 4]);
        assert_eq!(&*main[2], &[0.0; 4]);
        assert_eq!(&*key[0], &[0.375; 4]);
        assert!(
            validate_buses(
                &[Bus {
                    index: 1,
                    channels: 2
                }],
                &[Bus {
                    index: 0,
                    channels: 2
                }]
            )
            .is_err()
        );
    }
    #[test]
    fn state_container_rejects_wrong_component_truncation_and_trailing_bytes() {
        let state = StateContainer {
            component: component(),
            property_list: b"bplist00owned-state".to_vec(),
        };
        let bytes = state.encode().unwrap();
        assert_eq!(StateContainer::decode(&bytes, component()).unwrap(), state);
        assert!(StateContainer::decode(&bytes[..bytes.len() - 1], component()).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(StateContainer::decode(&extra, component()).is_err());
        let mut other = component();
        other.subtype += 1;
        assert!(StateContainer::decode(&bytes, other).is_err());
    }
    #[test]
    fn reset_requires_return_and_successful_control_side_recovery_before_render() {
        let mut phase = ProviderPhase::default();
        phase.activate().unwrap();
        phase.request_reset();
        assert!(!phase.render_allowed());
        assert!(phase.activate().is_err());
        phase.returned().unwrap();
        phase.recover(true);
        phase.activate().unwrap();
        assert!(phase.render_allowed());
        phase.request_reset();
        phase.returned().unwrap();
        phase.recover(false);
        assert_eq!(phase, ProviderPhase::Failed);
        assert!(phase.activate().is_err());
    }
    #[test]
    fn host_clock_maps_meter_beats_tempo_timeline_and_segment_downbeats() {
        let transport = crate::Transport {
            tempo_bpm: 120.0,
            position_beats: 4.25,
            position_seconds: 2.125,
            numerator: 7,
            denominator: 8,
            meter_anchor: Some(crate::MeterAnchor {
                bar_origin_beats: 4.0,
                bar_origin_index: 1,
            }),
            ..Default::default()
        };
        let clock = host_clock(&transport, 48_000.0).unwrap();
        assert_eq!(clock.next_beat_frames, 6000);
        assert_eq!(clock.downbeat, 4.0);
        assert_eq!(clock.timeline_sample, 102_000.0);
        assert_eq!(
            (clock.numerator, clock.denominator, clock.tempo),
            (7.0, 8, 120.0)
        );
        assert_eq!(
            host_clock(
                &crate::Transport {
                    position_beats: 4.5,
                    ..transport
                },
                48_000.0
            )
            .unwrap()
            .next_beat_frames,
            0
        );
        assert!(
            host_clock(
                &crate::Transport {
                    denominator: 3,
                    ..transport
                },
                48_000.0
            )
            .is_none()
        );
        assert!(host_clock(&transport, f64::NAN).is_none());
    }
    #[test]
    fn transport_change_detects_stop_and_seek_but_not_normal_progress() {
        let before = crate::Transport {
            playing: true,
            ..Default::default()
        };
        let after = crate::Transport {
            position_seconds: 0.01,
            position_beats: 0.02,
            ..before
        };
        assert!(!transport_changed(&before, &after, 480, 48_000.0));
        assert!(transport_changed(
            &before,
            &crate::Transport {
                playing: false,
                ..after
            },
            480,
            48_000.0
        ));
        assert!(transport_changed(
            &before,
            &crate::Transport {
                position_beats: 4.0,
                ..after
            },
            480,
            48_000.0
        ));
    }
}
