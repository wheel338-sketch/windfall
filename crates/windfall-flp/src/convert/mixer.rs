//! Mixer inserts to mixer tracks: faders, track processing, routing,
//! sends and effects.
//!
//! FL Studio always has its whole mixer in a file, 127 inserts from
//! version 12.9 on, used or not. Only the inserts that are in use are
//! brought over: the master, and any insert that has a name, an effect, a
//! fader, pan, mute, solo or track processing that was touched, routing
//! other than "to the master", or something playing into it. The last
//! insert of a full mixer is the "current" one, which follows whatever
//! is selected in FL Studio and is never brought over.
//!
//! Routing: an insert feeds any number of others, each at its own level.
//! A Windfall track has one output without a level, and sends with one.
//! So an insert that feeds the master at full level gets the master as
//! its output and its other routes as sends; an insert with a single
//! route at full level gets that as its output; and anything else gets no
//! output and a send for every route, which is the same signal flow.

use std::collections::BTreeSet;

use windfall_dsp::TrackParams;
use windfall_project::{
    Command, EffectSlotPatch, MAX_EFFECT_SLOTS, MAX_MIXER_SIGNAL_TRACKS, MixerTrackPatch, TrackId,
};

use super::{
    Builder, ImportedEffect, PluginPlace, PluginPlaceholder, chosen_color, effects, name_or,
};
use crate::model::{Insert, Route, Slot};
use crate::report::{Outcome, ReportSection, Worst};
use crate::units::{fader_gain, fader_is_exact, insert_pan};

/// The level of a route, a fader or a mix that was left alone.
const FULL: i32 = 12_800;

const FITTED_FADER: &str = "A fader or send level that is not at 0% or 100% went through a fitted curve, so it can be off by a little.";

impl Builder<'_> {
    pub(super) fn mixer(&mut self) {
        let flp = self.flp;
        let inserts = &flp.mixer.inserts;
        let current = current_insert(inserts.len());
        let used = self.inserts_in_use(current);
        if inserts.is_empty() && self.main_volume() != FULL {
            self.fader(&Insert::default(), 0);
        }

        let mut left_out = 0_u32;
        for &index in &used {
            let insert = &inserts[usize::from(index)];
            if index == 0 {
                self.fader(insert, index);
                continue;
            }
            if self.tracks.len() >= MAX_MIXER_SIGNAL_TRACKS {
                left_out += 1;
                continue;
            }
            let name = name_or(insert.name.as_deref(), || format!("Insert {index}"));
            let command = Command::AddMixerTrack { name: Some(name) };
            if let Some(id) = self.create(ReportSection::Mixer, "A mixer track", command) {
                self.tracks.insert(index, TrackId(id));
                self.fader(insert, index);
            }
        }
        if left_out > 0 {
            self.report
                .count(ReportSection::Mixer, Outcome::Dropped, left_out);
            self.report.say(
                ReportSection::Mixer,
                Outcome::Dropped,
                format!(
                    "Windfall's mixer holds {MAX_MIXER_SIGNAL_TRACKS} tracks. {left_out} more were left out, and what played into them plays into the master."
                ),
            );
        }
        if let Some(current) = current
            && inserts
                .get(usize::from(current))
                .is_some_and(|insert| !insert.slots.is_empty())
        {
            self.report.say(
                ReportSection::Mixer,
                Outcome::Dropped,
                "The effects on FL Studio's \"current\" mixer track, which follows the selection, were left out.",
            );
        }

        for &index in &used {
            if index != 0 && self.tracks.contains_key(&index) {
                self.routing(&inserts[usize::from(index)], index, current);
            }
        }
        for &index in &used {
            if self.tracks.contains_key(&index) {
                self.effects(&inserts[usize::from(index)], index);
            }
        }
    }

    /// The inserts that are brought over, in order.
    fn inserts_in_use(&self, current: Option<u16>) -> Vec<u16> {
        let inserts = &self.flp.mixer.inserts;
        let mut used: BTreeSet<u16> = BTreeSet::from([0]);
        for (index, insert) in inserts.iter().enumerate() {
            if touched(insert) {
                used.insert(index as u16);
            }
        }
        for channel in &self.flp.channels {
            if let Some(insert) = channel.insert.and_then(|insert| u16::try_from(insert).ok()) {
                used.insert(insert);
            }
        }
        // An insert that something in use feeds is in use too.
        let mut frontier: Vec<u16> = used.iter().copied().collect();
        while let Some(index) = frontier.pop() {
            let Some(insert) = inserts.get(usize::from(index)) else {
                continue;
            };
            for route in &insert.routes {
                if used.insert(route.target) {
                    frontier.push(route.target);
                }
            }
        }
        used.into_iter()
            .filter(|&index| usize::from(index) < inserts.len() && Some(index) != current)
            .collect()
    }

    /// Sets the name, colour, fader, pan, mute, solo and processing of the
    /// track an insert became, and says what could not be decoded.
    fn fader(&mut self, insert: &Insert, index: u16) {
        let section = ReportSection::Mixer;
        let Some(&track) = self.tracks.get(&index) else {
            return;
        };
        let mut worst = Worst::new();
        let name = name_or(insert.name.as_deref(), || {
            self.project()
                .mixer
                .track(track)
                .map_or_else(String::new, |track| track.name.clone())
        });
        let raw = insert.volume.unwrap_or(FULL);
        let mut volume = fader_gain(raw);
        if !fader_is_exact(raw) {
            worst.note(Outcome::Approximated);
            self.report
                .say(section, Outcome::Approximated, FITTED_FADER);
        }
        if index == 0 {
            // FL Studio's main volume comes after the master insert. The
            // master's fader takes both.
            let main = self.main_volume();
            if main != FULL {
                volume *= fader_gain(main);
                worst.note(Outcome::Approximated);
                self.report.say(
                    section,
                    Outcome::Approximated,
                    "The project's main volume was folded into the master track's fader.",
                );
            }
        }
        let patch = MixerTrackPatch {
            name: (index != 0 || !name.eq_ignore_ascii_case("master")).then(|| name.clone()),
            color: chosen_color(insert.color),
            volume: Some(volume),
            pan: Some(insert_pan(insert.pan.unwrap_or(0))),
            muted: Some(!insert.enabled()),
            solo: Some(insert.solo()),
            processing: Some(track_processing(insert)),
            recording: None,
            dock: insert.flags.map(|flags| {
                if flags & (1 << 6) != 0 {
                    windfall_project::MixerDock::Middle
                } else if flags & (1 << 7) != 0 {
                    windfall_project::MixerDock::Right
                } else {
                    windfall_project::MixerDock::Left
                }
            }),
            ..MixerTrackPatch::default()
        };
        let command = Command::UpdateMixerTrack { id: track, patch };
        if self
            .apply(
                section,
                &format!("The settings of the mixer track \"{name}\""),
                command,
            )
            .is_none()
        {
            worst.note(Outcome::Dropped);
        }

        if insert.stereo_separation.is_some_and(|amount| amount != 0) {
            worst.note(Outcome::Approximated);
            self.report.say(
                section,
                Outcome::Approximated,
                format!(
                    "Mixer track \"{name}\": stereo separation was mapped linearly from -64..64 to -1..1; -1 doubles the side signal and +1 sums to mono. FL Studio's listening direction is unverified."
                ),
            );
        }
        if has_eq_gain(insert) || has_eq_shape(insert) {
            worst.note(Outcome::Approximated);
            self.report.say(
                section,
                Outcome::Approximated,
                format!(
                    "Mixer track \"{name}\": the insert EQ frequency and width were not decoded; Windfall's default frequencies and Q values were kept."
                ),
            );
        }
        self.report.count(section, worst.0, 1);
    }

    /// The main volume as FL Studio's number, where 12800 is 100%.
    fn main_volume(&self) -> i32 {
        let flp = self.flp;
        flp.mixer.main_volume().unwrap_or_else(|| {
            // Older files have it as a byte, where 128 is full.
            flp.settings
                .main_volume
                .map_or(FULL, |volume| i32::from(volume) * 100)
        })
    }

    fn routing(&mut self, insert: &Insert, index: u16, current: Option<u16>) {
        let section = ReportSection::Mixer;
        let Some(&track) = self.tracks.get(&index) else {
            return;
        };
        let name = self.track_name(track);
        for route in &insert.routes {
            if route.target == index {
                self.loop_refused(&name, route.target);
            } else if !self.tracks.contains_key(&route.target) || Some(route.target) == current {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    "A mixer route points to an unavailable insert and was left out.",
                );
            }
        }
        let routes: Vec<Route> = insert
            .routes
            .iter()
            .copied()
            .filter(|route| {
                route.target != index
                    && Some(route.target) != current
                    && self.tracks.contains_key(&route.target)
            })
            .collect();
        let full = |route: &Route| route.level.unwrap_or(FULL) == FULL;
        let output = routes
            .iter()
            .find(|route| route.target == 0 && full(route))
            .or_else(|| {
                routes
                    .first()
                    .filter(|route| routes.len() == 1 && full(route))
            })
            .map(|route| route.target);

        let mut output_set = false;
        if let Some(target) = output {
            let command = Command::SetTrackOutput {
                id: track,
                output: Some(self.tracks[&target]),
            };
            match self.run(command) {
                Ok(_) => output_set = true,
                Err(_) => self.loop_refused(&name, target),
            }
        }
        if !output_set {
            // A new track goes to the master, and this one does not.
            let command = Command::SetTrackOutput {
                id: track,
                output: None,
            };
            self.apply(
                section,
                &format!("The output of the mixer track \"{name}\""),
                command,
            );
        }
        for route in routes {
            if output_set && Some(route.target) == output {
                continue;
            }
            let level = route.level.unwrap_or(FULL);
            // Public parsers represent sidechain-only connections as enabled
            // routes at zero audible level. Keep the detector route at unity.
            let command = if level == 0 {
                Command::SetSidechain {
                    from: track,
                    to: self.tracks[&route.target],
                    gain: Some(1.0),
                }
            } else {
                Command::SetSend {
                    from: track,
                    to: self.tracks[&route.target],
                    gain: Some(fader_gain(level)),
                }
            };
            match self.run(command) {
                Ok(_) => {
                    if level != 0 {
                        self.sends.insert((index, route.target));
                    } else {
                        self.report.say(section, Outcome::Approximated, "A zero-level enabled route became a unity detector-only sidechain. Dynamic transitions between audible send and sidechain require review.");
                    }
                    if !fader_is_exact(level) {
                        self.report
                            .say(section, Outcome::Approximated, FITTED_FADER);
                    }
                }
                Err(_) => self.loop_refused(&name, route.target),
            }
        }
    }

    fn loop_refused(&mut self, from: &str, target: u16) {
        let to = self
            .tracks
            .get(&target)
            .map_or_else(String::new, |&track| self.track_name(track));
        self.report.count(ReportSection::Mixer, Outcome::Dropped, 1);
        self.report.say(
            ReportSection::Mixer,
            Outcome::Dropped,
            format!(
                "The route from \"{from}\" to \"{to}\" would feed a mixer track back into itself, which Windfall does not allow. It was left out."
            ),
        );
    }

    pub(super) fn track_name(&self, track: TrackId) -> String {
        self.project()
            .mixer
            .track(track)
            .map_or_else(String::new, |track| track.name.clone())
    }

    fn effects(&mut self, insert: &Insert, index: u16) {
        let Some(&track) = self.tracks.get(&index) else {
            return;
        };
        let track_name = self.track_name(track);
        let mut slots: Vec<&Slot> = insert.slots.iter().collect();
        slots.sort_by_key(|slot| slot.index);
        for slot in slots {
            self.effect(insert, index, track, &track_name, slot);
        }
    }

    fn effect(
        &mut self,
        insert: &Insert,
        index: u16,
        track: TrackId,
        track_name: &str,
        slot: &Slot,
    ) {
        let section = ReportSection::Effects;
        let plugin = &slot.plugin;
        let hosted = plugin.hosted();
        let shown = name_or(slot.name.as_deref(), || {
            hosted
                .as_ref()
                .and_then(|hosted| hosted.name.clone())
                .unwrap_or_else(|| plugin.internal_name.clone())
        });

        self.plugins.push(PluginPlaceholder {
            place: PluginPlace::Effect {
                track,
                slot: slot.index,
            },
            internal_name: plugin.internal_name.clone(),
            name: hosted.as_ref().and_then(|hosted| hosted.name.clone()),
            vendor: hosted.as_ref().and_then(|hosted| hosted.vendor.clone()),
            format: hosted.as_ref().map(|hosted| hosted.format),
            path: hosted.as_ref().and_then(|hosted| hosted.path.clone()),
            state: plugin.state.clone(),
        });

        let Some(translated) =
            effects::translate(&plugin.internal_name, &plugin.state, self.tempo_bpm)
        else {
            self.report.count(section, Outcome::Placeholder, 1);
            let what = if plugin.is_hosted() {
                let format = hosted
                    .as_ref()
                    .map_or(PluginPlaceholderFormat::UNKNOWN, |hosted| {
                        hosted.format.label()
                    });
                let vendor = hosted
                    .as_ref()
                    .and_then(|hosted| hosted.vendor.as_deref())
                    .map_or_else(String::new, |vendor| format!(" by {vendor}"));
                format!(
                    "Mixer track \"{track_name}\": the plugin \"{shown}\"{vendor} ({format}) is not one Windfall can load yet. Its place and its settings were kept for when it can."
                )
            } else {
                format!(
                    "Mixer track \"{track_name}\": the effect \"{shown}\" has no Windfall equivalent. It was left out of the chain, and its settings were kept."
                )
            };
            self.report.say(section, Outcome::Placeholder, what);
            return;
        };

        let chain = self
            .project()
            .mixer
            .track(track)
            .map_or(0, |track| track.effects.len());
        if chain >= MAX_EFFECT_SLOTS {
            self.report.count(section, Outcome::Dropped, 1);
            self.report.say(
                section,
                Outcome::Dropped,
                format!(
                    "Mixer track \"{track_name}\": a track holds {MAX_EFFECT_SLOTS} effects, so \"{shown}\" was left out."
                ),
            );
            return;
        }
        let kind = translated.params.kind();
        let what = format!("The effect \"{shown}\" on the mixer track \"{track_name}\"");
        let command = Command::AddEffect {
            track,
            kind,
            index: None,
        };
        let Some(created) = self.apply(section, &what, command) else {
            self.report.count(section, Outcome::Dropped, 1);
            return;
        };
        let id = windfall_project::EffectId(created[0]);
        let command = Command::SetEffectParams {
            track,
            effect: id,
            params: translated.params,
        };
        self.apply(
            section,
            &format!("The settings of {}", lower_first(&what)),
            command,
        );
        if kind == windfall_dsp::EffectKind::SendTap && plugin.state.len() == 16 {
            let destination = i32::from_le_bytes(
                plugin.state[12..16]
                    .try_into()
                    .expect("checked state length"),
            );
            // PyFLP documents an insert index, but notes it depends on routing.
            // Only an explicit nonnegative index is safe to associate here.
            if let Ok(destination) = u16::try_from(destination)
                && let Some(&target) = self.tracks.get(&destination)
            {
                self.apply(
                    section,
                    "The send tap destination",
                    Command::SetEffectOutput {
                        effect: id,
                        output: 0,
                        target: Some(target),
                    },
                );
            } else {
                self.report.say(section, Outcome::Dropped, "The send tap destination is relative, negative or unavailable and was not guessed.");
            }
            let volume = u32::from_le_bytes(
                plugin.state[8..12]
                    .try_into()
                    .expect("checked state length"),
            );
            self.apply(
                section,
                "The send tap level",
                Command::SetEffectParams {
                    track,
                    effect: id,
                    params: windfall_dsp::EffectParams::SendTap(windfall_dsp::sendtap::SendTapParams {
                        level: (volume as f32 / 256.0).clamp(0.0, 1.0),
                        ..Default::default()
                    }),
                },
            );
            self.report.say(section, Outcome::Approximated, "Send tap level uses a linear approximation of the source logarithmic level; source pan and dry controls are not mapped.");
        }
        let mix = slot.mix.unwrap_or(FULL);
        let patch = EffectSlotPatch {
            // Switching all of an insert's slots off is one button in FL
            // Studio; here every effect of the track is switched off.
            enabled: Some(slot.enabled.unwrap_or(true) && insert.effects_enabled()),
            mix: Some((mix as f32 / FULL as f32).clamp(0.0, 1.0)),
        };
        let command = Command::UpdateEffect {
            track,
            effect: id,
            patch,
        };
        self.apply(
            section,
            &format!("The mix of {}", lower_first(&what)),
            command,
        );

        let outcome = Outcome::Approximated;
        self.report.say(section, outcome, "Mapped effects use a different processor and may sound different even when their control values translate exactly.");
        self.report.count(section, outcome, 1);
        if !translated.notes.is_empty() {
            self.report.say(
                section,
                Outcome::Approximated,
                format!(
                    "Mixer track \"{track_name}\": \"{shown}\" became a Windfall {}: {}.",
                    kind.name().to_lowercase(),
                    translated.notes.join("; ")
                ),
            );
        }
        self.effects.insert(
            (index, u16::from(slot.index)),
            ImportedEffect {
                track,
                id,
                kind,
                links: translated.links,
            },
        );
    }
}

/// Words for a plugin format the wrapper did not give.
struct PluginPlaceholderFormat;

impl PluginPlaceholderFormat {
    const UNKNOWN: &'static str = "an unknown format";
}

/// True for an insert somebody did something with.
fn touched(insert: &Insert) -> bool {
    let named = insert
        .name
        .as_deref()
        .is_some_and(|name| !name.trim().is_empty());
    let moved = insert.volume.is_some_and(|volume| volume != FULL)
        || insert.pan.is_some_and(|pan| pan != 0);
    let to_master_only = insert.routes.len() == 1
        && insert.routes[0].target == 0
        && insert.routes[0].level.unwrap_or(FULL) == FULL;
    let rerouted = !insert.routes.is_empty() && !to_master_only;
    named
        || moved
        || rerouted
        || !insert.slots.is_empty()
        || !insert.enabled()
        || insert.solo()
        || !track_processing(insert).is_default()
        || has_eq_shape(insert)
}

fn has_eq_gain(insert: &Insert) -> bool {
    insert
        .eq
        .iter()
        .any(|band| band.gain.is_some_and(|gain| gain != 0))
}

fn has_eq_shape(insert: &Insert) -> bool {
    insert
        .eq
        .iter()
        .any(|band| band.frequency.is_some() || band.width.is_some())
}

/// The insert's one polarity switch inverts both channels. Separation
/// uses raw / 64: -1 doubles side, +1 sums to mono in TrackParams. The
/// source's listening direction has not been verified.
fn track_processing(insert: &Insert) -> TrackParams {
    let mut params = TrackParams {
        invert_left: insert.polarity_reversed(),
        invert_right: insert.polarity_reversed(),
        swap: insert.channels_swapped(),
        separation: (insert.stereo_separation.unwrap_or(0) as f32 / 64.0).clamp(-1.0, 1.0),
        ..TrackParams::default()
    };
    // Preserve all defaults for a neutral insert, including the enabled
    // switches. A zero-gain enabled band leaves the signal unchanged.
    if has_eq_gain(insert) {
        params.eq_enabled = true;
        for (source, target) in
            insert
                .eq
                .iter()
                .zip([&mut params.low, &mut params.mid, &mut params.high])
        {
            // TrackParams gives each band's gain the range -24..24 dB.
            target.gain_db = (source.gain.unwrap_or(0) as f32 / 100.0).clamp(-24.0, 24.0);
            target.enabled = target.gain_db != 0.0;
            // effects.rs has distinct curves for the two Fruity EQ
            // plugins, but no established curve for this insert EQ.
            // Frequency and Q therefore stay at TrackParams defaults.
        }
    }
    params
}

/// The number of FL Studio's "current" insert, the last of a full mixer.
///
/// PyFLP lists the sizes the mixer has had: 105 inserts from version 9 and
/// 127 from 12.9, each with the master first and the current insert last.
/// A file with another number of inserts is a preset or a fixture, and has
/// none.
fn current_insert(count: usize) -> Option<u16> {
    matches!(count, 105 | 127).then(|| (count - 1) as u16)
}

fn lower_first(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().chain(characters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{Conversion, ConvertOptions, convert};
    use crate::model::{FlpProject, InsertEqBand};

    fn import_insert(insert: Insert) -> Conversion {
        let mut source = FlpProject::default();
        source.mixer.inserts = vec![Insert::default(), insert];
        let imported = convert(&source, &ConvertOptions::default());
        imported.project.check().expect("valid imported project");
        assert_eq!(imported.project.mixer.tracks.len(), 2);
        imported
    }

    #[test]
    fn polarity_reversal_inverts_both_channels() {
        // Leave mute and effect-slot enable switches on.
        let imported = import_insert(Insert {
            flags: Some(0xc | 0x1),
            ..Insert::default()
        });
        assert_eq!(
            imported.project.mixer.tracks[1].processing,
            TrackParams {
                invert_left: true,
                invert_right: true,
                ..TrackParams::default()
            }
        );
        assert!(
            imported
                .report
                .category(ReportSection::Mixer)
                .lines
                .is_empty()
        );
    }

    #[test]
    fn swapped_channels_set_the_track_switch() {
        let imported = import_insert(Insert {
            flags: Some(0xc | 0x2),
            ..Insert::default()
        });
        assert_eq!(
            imported.project.mixer.tracks[1].processing,
            TrackParams {
                swap: true,
                ..TrackParams::default()
            }
        );
        assert!(
            imported
                .report
                .category(ReportSection::Mixer)
                .lines
                .is_empty()
        );
    }

    #[test]
    fn separation_is_linear_and_clamped_at_both_endpoints() {
        for (raw, expected) in [
            (-128, -1.0),
            (-64, -1.0),
            (-32, -0.5),
            (0, 0.0),
            (32, 0.5),
            (64, 1.0),
            (128, 1.0),
        ] {
            let imported = import_insert(Insert {
                name: Some("Stereo".into()),
                stereo_separation: Some(raw),
                ..Insert::default()
            });
            assert_eq!(
                imported.project.mixer.tracks[1].processing.separation, expected,
                "source separation {raw}"
            );
            let lines = &imported.report.category(ReportSection::Mixer).lines;
            if raw == 0 {
                assert!(lines.is_empty());
            } else {
                assert!(lines.iter().any(|line| {
                    line.outcome == Outcome::Approximated
                        && line.text.contains("listening direction is unverified")
                }));
            }
        }
    }

    #[test]
    fn insert_eq_gains_are_hundredths_of_db_in_low_mid_high_order() {
        let imported = import_insert(Insert {
            eq: [625, -1800, 1800].map(|gain| InsertEqBand {
                gain: Some(gain),
                frequency: Some(65_536),
                width: Some(0),
            }),
            ..Insert::default()
        });
        let params = imported.project.mixer.tracks[1].processing;
        let defaults = TrackParams::default();
        assert!(params.eq_enabled);
        for (band, default, gain_db) in [
            (params.low, defaults.low, 6.25),
            (params.mid, defaults.mid, -18.0),
            (params.high, defaults.high, 18.0),
        ] {
            assert!(band.enabled);
            assert_eq!(band.gain_db, gain_db);
            assert_eq!(band.frequency_hz, default.frequency_hz);
            assert_eq!(band.q, default.q);
        }
    }

    #[test]
    fn imported_gains_clamp_to_track_ranges_and_only_nonzero_bands_are_enabled() {
        let imported = import_insert(Insert {
            eq: [
                InsertEqBand {
                    gain: Some(i32::MIN),
                    ..InsertEqBand::default()
                },
                InsertEqBand {
                    gain: Some(0),
                    ..InsertEqBand::default()
                },
                InsertEqBand {
                    gain: Some(i32::MAX),
                    ..InsertEqBand::default()
                },
            ],
            ..Insert::default()
        });
        let params = imported.project.mixer.tracks[1].processing;
        assert!(params.eq_enabled);
        assert!(params.low.enabled);
        assert_eq!(params.low.gain_db, -24.0);
        assert!(!params.mid.enabled);
        assert_eq!(params.mid.gain_db, 0.0);
        assert!(params.high.enabled);
        assert_eq!(params.high.gain_db, 24.0);
    }

    #[test]
    fn neutral_inserts_keep_default_processing_without_a_new_report_line() {
        for insert in [
            Insert::default(),
            Insert {
                flags: Some(0xc),
                stereo_separation: Some(0),
                eq: [InsertEqBand {
                    gain: Some(0),
                    ..InsertEqBand::default()
                }; 3],
                ..Insert::default()
            },
        ] {
            assert!(!touched(&insert));
            let imported = import_insert(Insert {
                name: Some("Neutral".into()),
                ..insert
            });
            assert_eq!(
                imported.project.mixer.tracks[1].processing,
                TrackParams::default()
            );
            assert!(
                imported
                    .report
                    .category(ReportSection::Mixer)
                    .lines
                    .is_empty()
            );
        }
    }

    #[test]
    fn eq_import_reports_undecoded_shape_without_the_old_unsupported_sentence() {
        let imported = import_insert(Insert {
            eq: [
                InsertEqBand {
                    gain: Some(300),
                    ..InsertEqBand::default()
                },
                InsertEqBand::default(),
                InsertEqBand::default(),
            ],
            ..Insert::default()
        });
        let params = imported.project.mixer.tracks[1].processing;
        assert_eq!(params.low.gain_db, 3.0);
        assert!(params.low.enabled);
        assert!(!params.mid.enabled);
        assert!(!params.high.enabled);
        let category = imported.report.category(ReportSection::Mixer);
        assert_eq!(category.approximated, 1);
        assert!(category.lines.iter().any(|line| {
            line.text.contains("frequency and width were not decoded")
                && line.outcome == Outcome::Approximated
        }));
        assert!(imported.report.categories.iter().all(|category| {
            category.lines.iter().all(|line| {
                !line.text.contains("Windfall's tracks have no")
                    && !line.text.contains("no three-band equaliser")
            })
        }));
    }

    #[test]
    fn undecoded_eq_shape_without_gain_is_reported_and_keeps_default_processing() {
        let imported = import_insert(Insert {
            eq: [InsertEqBand {
                frequency: Some(32_768),
                width: Some(32_768),
                ..InsertEqBand::default()
            }; 3],
            ..Insert::default()
        });
        assert_eq!(
            imported.project.mixer.tracks[1].processing,
            TrackParams::default()
        );
        assert!(
            imported
                .report
                .category(ReportSection::Mixer)
                .lines
                .iter()
                .any(|line| line.text.contains("frequency and width were not decoded"))
        );
    }

    #[test]
    fn master_insert_uses_the_same_track_processing() {
        let mut source = FlpProject::default();
        source.mixer.inserts.push(Insert {
            flags: Some(0xf),
            stereo_separation: Some(64),
            ..Insert::default()
        });
        let imported = convert(&source, &ConvertOptions::default());
        let params = imported.project.mixer.tracks[0].processing;
        assert!(params.invert_left && params.invert_right && params.swap);
        assert_eq!(params.separation, 1.0);
    }
}
