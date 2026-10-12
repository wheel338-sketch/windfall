//! Notes retain their timing at the source PPQ. Rounding and unsupported
//! note expression are reported rather than hidden.
use super::{Builder, ChannelRole, chosen_color, name_or};
use crate::{
    report::{Outcome, ReportSection},
    units,
};
use std::collections::{BTreeMap, BTreeSet};
use windfall_core::TICKS_PER_STEP;
use windfall_project::{
    AutomationPoint, ChannelId, Command, MAX_AUTOMATION_POINTS, MAX_PATTERN_STEPS,
    MAX_PATTERN_TICKS, NoteInit, PatternId, PatternPatch,
};

impl Builder<'_> {
    fn recorded_controls(&mut self, pattern: PatternId, events: &[crate::model::ControlEvent]) {
        let section = ReportSection::Automation;
        // Bound work before building lanes. Last source record at a rounded tick
        // wins; held points reproduce discrete recorded control changes.
        let mut groups: BTreeMap<u32, Vec<&crate::model::ControlEvent>> = BTreeMap::new();
        let budget = crate::parse::MAX_RECORDED_CONTROL_EVENTS;
        if events.len() > budget {
            self.report.count(section, Outcome::Dropped, (events.len() - budget).min(u32::MAX as usize) as u32);
            self.report.say(section, Outcome::Dropped, "Recorded controls exceed the import event budget.");
        }
        for event in events.iter().take(budget) {
            groups.entry(event.location).or_default().push(event);
        }
        for (location, events) in groups {
            let source = crate::target::ControlTarget::decode(location);
            let Some(target) = self.automation_target(source) else {
                self.report.count(section, Outcome::Dropped, events.len() as u32);
                self.report.say(section, Outcome::Dropped,
                    format!("Recorded pattern target {location:#010x} is unsupported; source event records remain in the FLP reader model."));
                continue;
            };
            let mut points = BTreeMap::new();
            let mut accepted = 0;
            for event in &events {
                let tick = units::rescale(event.position, self.ppq).ticks;
                let Some(value) = self.recorded_value(source, &target, event.value) else { continue; };
                if tick > u64::from(MAX_PATTERN_TICKS) ||
                    (points.len() >= MAX_AUTOMATION_POINTS && !points.contains_key(&(tick as u32))) {
                    continue;
                }
                points.insert(tick as u32, AutomationPoint { tick: tick as u32, value, curve: 0.0, hold: true });
                accepted += 1;
            }
            if accepted != 0 && self.apply(section, "Recorded pattern automation", Command::SetPatternEvents {
                pattern, target, expected: Vec::new(), points: points.into_values().collect(),
            }).is_some() {
                self.report.count(section, Outcome::Approximated, accepted);
                self.report.say(section, Outcome::Approximated,
                    "Recorded control integers were mapped to held pattern events; source interpolation and nonlinear control tapers may differ.");
            } else { accepted = 0; }
            self.report.count(section, Outcome::Dropped, events.len() as u32 - accepted);
        }
    }

    pub(super) fn note_channels(&self, iid: u16) -> Vec<ChannelId> {
        let mut seen = BTreeSet::new();
        let mut pending = vec![iid];
        let mut result = BTreeSet::new();
        while let Some(iid) = pending.pop() {
            if !seen.insert(iid) {
                continue;
            }
            if let Some(&id) = self.note_fallbacks.get(&iid) {
                result.insert(id);
            }
            match self.channels.get(&iid) {
                Some(ChannelRole::Plays(id)) => {
                    result.insert(*id);
                }
                Some(ChannelRole::Layer(children)) => pending.extend(children),
                _ => {}
            }
        }
        result.into_iter().collect()
    }

    pub(super) fn patterns(&mut self) {
        for (index, pattern) in self.flp.patterns.iter().enumerate() {
            let id = if index == 0 {
                self.project().patterns[0].id
            } else {
                let Some(id) = self.create(
                    ReportSection::Patterns,
                    "A pattern",
                    Command::AddPattern { name: None },
                ) else {
                    continue;
                };
                PatternId(id)
            };
            let notes_end = pattern
                .notes
                .iter()
                .map(|n| {
                    u64::from(n.position) + u64::from(n.length.max(u32::from(self.ppq) / 4).max(1))
                })
                .max()
                .unwrap_or(u64::from(self.ppq) * 4);
            let source_length = pattern
                .length
                .filter(|l| *l != 0)
                .map(u64::from)
                .unwrap_or(notes_end);
            let scaled = units::rescale(source_length.min(u64::from(u32::MAX)) as u32, self.ppq);
            let steps = scaled
                .ticks
                .div_ceil(u64::from(TICKS_PER_STEP))
                .clamp(1, u64::from(MAX_PATTERN_STEPS)) as u32;
            self.apply(
                ReportSection::Patterns,
                "Pattern settings",
                Command::UpdatePattern {
                    id,
                    patch: PatternPatch {
                        name: Some(name_or(pattern.name.as_deref(), || {
                            format!("Pattern {}", pattern.iid)
                        })),
                        color: chosen_color(pattern.color),
                        length_steps: Some(steps),
                    },
                },
            );
            self.patterns
                .insert(pattern.iid, (id, steps * TICKS_PER_STEP));
            self.pattern_timeline(id, &pattern.markers);
            self.recorded_controls(id, &pattern.control_events);
            let outcome = if scaled.exact && scaled.ticks == u64::from(steps * TICKS_PER_STEP) {
                Outcome::Exact
            } else {
                Outcome::Approximated
            };
            self.report.count(ReportSection::Patterns, outcome, 1);
            if outcome != Outcome::Exact {
                self.report.say(
                    ReportSection::Patterns,
                    outcome,
                    "Pattern length was rounded to Windfall's step grid or limited to 1024 steps.",
                );
            }
            for note in &pattern.notes {
                let targets = self.note_channels(note.channel);
                let start = units::rescale(note.position, self.ppq);
                let length = if note.length == 0 {
                    units::Rescaled {
                        ticks: u64::from(TICKS_PER_STEP),
                        exact: true,
                    }
                } else {
                    units::rescale(note.length, self.ppq)
                };
                if targets.is_empty()
                    || start.ticks >= u64::from(MAX_PATTERN_TICKS)
                    || note.key > 127
                {
                    self.report.count(ReportSection::Notes, Outcome::Dropped, 1);
                    self.report.say(ReportSection::Notes, Outcome::Dropped, "A note has no playable channel, an unsupported key, or starts past the longest pattern.");
                    continue;
                }
                let mut outcome = if start.exact
                    && length.exact
                    && length.ticks <= u64::from(MAX_PATTERN_TICKS)
                    && targets.len() == 1
                {
                    Outcome::Exact
                } else {
                    Outcome::Approximated
                };
                if note.fine_pitch > 240
                    || note.flags & 8 != 0
                    || note.mod_x != 128
                    || note.mod_y != 128
                    || note.release != 64
                    || note.midi_channel > 15
                    || note.group != 0
                    || note.velocity > 128
                    || note.pan > 128
                {
                    outcome = Outcome::Approximated;
                    self.report.say(ReportSection::Notes, outcome, "Modulation, release, slide flags and note-color MIDI channels were mapped to Windfall's controls. Source note-edit groups are not translated; out-of-range levels, color channels and fine pitch were clamped. Slide chord targeting and other source-instrument behavior may differ.");
                }
                for channel in targets {
                    let init = NoteInit {
                        start: start.ticks as u32,
                        length: length.ticks.clamp(1, u64::from(MAX_PATTERN_TICKS)) as u32,
                        key: note.key as u8,
                        velocity: Some(units::note_velocity(note.velocity)),
                        pan: Some(units::note_pan(note.pan)),
                        expression: Some(windfall_project::NoteExpression {
                            release: f32::from(note.release.min(128)) / 128.0,
                            fine_pitch_cents: (f32::from(note.fine_pitch.min(240)) - 120.0) * 10.0,
                            modulation_x: f32::from(note.mod_x) / 256.0,
                            modulation_y: f32::from(note.mod_y) / 256.0,
                            articulation: if note.flags & 8 != 0 {
                                windfall_dsp::NoteArticulation::Slide
                            } else {
                                windfall_dsp::NoteArticulation::Normal
                            },
                            glide_ticks: length.ticks.clamp(1, u64::from(MAX_PATTERN_TICKS)) as u32,
                            color_group: Some(note.midi_channel.min(15)),
                        }),
                    };
                    if self
                        .apply(
                            ReportSection::Notes,
                            "A note",
                            Command::AddNotes {
                                pattern: id,
                                channel,
                                notes: vec![init],
                            },
                        )
                        .is_none()
                    {
                        outcome = Outcome::Dropped;
                    }
                }
                self.report.count(ReportSection::Notes, outcome, 1);
            }
            for step in &pattern.legacy_steps {
                let targets = self.note_channels(step.channel);
                for channel in targets {
                    self.apply(
                        ReportSection::Notes,
                        "A legacy step",
                        Command::AddNotes {
                            pattern: id,
                            channel,
                            notes: vec![NoteInit {
                                start: u32::from(step.step()) * TICKS_PER_STEP,
                                length: TICKS_PER_STEP,
                                key: 60,
                                velocity: None,
                                pan: None,
                                expression: None,
                            }],
                        },
                    );
                }
                self.report
                    .count(ReportSection::Notes, Outcome::Approximated, 1);
            }
        }
    }
}
