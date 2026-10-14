//! What an import did, in words a user can read.
//!
//! A report has a fixed row of categories. Each counts the things of its
//! kind by how they came across, and lists what there is to say about
//! them. The counts are of things (channels, notes, clips), one outcome
//! each: the worst of what happened to it. The lines are the details, and
//! a line that applies to several things says to how many.
//!
//! These types serialize to camelCase JSON and derive `TS`, so the shell
//! can hand a report to the interface as it is. They are not exported to
//! the bindings folder until the shell uses them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Most lines one category keeps. A project with hundreds of plugins this
/// crate has no equivalent for would otherwise bury everything else.
pub const MAX_LINES_PER_CATEGORY: usize = 200;

/// How something came across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// It is in the Windfall project as it was in the FL Studio project.
    Exact,
    /// It is there, and something about it is not the same: a level went
    /// through a fitted curve, a setting had nowhere to go.
    Approximated,
    /// Something stands in its place so that what depends on it survives,
    /// as a silent channel does for an instrument Windfall does not have.
    Placeholder,
    /// It is not in the Windfall project.
    Dropped,
}

/// The parts of a project a report is divided into, in the order a report
/// lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReportSection {
    /// Tempo, time signature, name and the other project settings.
    Project,
    /// The channel rack: samplers, instruments and what stands in for the
    /// rest.
    Channels,
    /// The audio files the project uses.
    Samples,
    /// Patterns.
    Patterns,
    /// Notes and steps.
    Notes,
    /// Mixer tracks, their faders and their routing.
    Mixer,
    /// Effects on mixer tracks.
    Effects,
    /// Playlist tracks and clips.
    Playlist,
    /// Automation clips.
    Automation,
    /// Markers, other arrangements and whatever else has no place above.
    Other,
}

impl ReportSection {
    pub const ALL: [ReportSection; 10] = [
        ReportSection::Project,
        ReportSection::Channels,
        ReportSection::Samples,
        ReportSection::Patterns,
        ReportSection::Notes,
        ReportSection::Mixer,
        ReportSection::Effects,
        ReportSection::Playlist,
        ReportSection::Automation,
        ReportSection::Other,
    ];

    /// The heading to show.
    pub fn title(self) -> &'static str {
        match self {
            ReportSection::Project => "Project settings",
            ReportSection::Channels => "Channels",
            ReportSection::Samples => "Samples",
            ReportSection::Patterns => "Patterns",
            ReportSection::Notes => "Notes",
            ReportSection::Mixer => "Mixer",
            ReportSection::Effects => "Effects",
            ReportSection::Playlist => "Playlist",
            ReportSection::Automation => "Automation",
            ReportSection::Other => "Everything else",
        }
    }
}

/// One category of a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportCategory {
    pub section: ReportSection,
    pub title: String,
    /// Things that came across as they were.
    pub exact: u32,
    pub approximated: u32,
    pub placeholders: u32,
    pub dropped: u32,
    /// What there is to say, in the order it came up. Lines with the same
    /// words are one line with a count.
    pub lines: Vec<ReportLine>,
    /// Lines left out because the category already had
    /// [`MAX_LINES_PER_CATEGORY`].
    pub more_lines: u32,
}

impl ReportCategory {
    fn new(section: ReportSection) -> Self {
        Self {
            section,
            title: section.title().to_owned(),
            exact: 0,
            approximated: 0,
            placeholders: 0,
            dropped: 0,
            lines: Vec::new(),
            more_lines: 0,
        }
    }

    /// How many things the category counted.
    pub fn total(&self) -> u32 {
        self.exact
            .saturating_add(self.approximated)
            .saturating_add(self.placeholders)
            .saturating_add(self.dropped)
    }
}

/// One thing a report says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportLine {
    pub outcome: Outcome,
    /// How many times this came up.
    pub count: u32,
    pub text: String,
}

/// The controls of one device that were read from the project and left out,
/// because the Windfall device that stands in for it has no matching control.
/// The device's original state is kept with the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OmittedControls {
    /// The device as the project names it, such as "Fruity Limiter".
    pub device: String,
    /// How many of this device the project uses.
    pub instances: u32,
    /// The names of the omitted fields, in the order of the device's layout.
    pub fields: Vec<String>,
}

/// Devices a report lists in [`ImportReport::omitted_controls`] at most.
pub const MAX_OMITTED_DEVICES: usize = 256;

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// The version of FL Studio that saved the file, as the file gives it.
    #[ts(optional)]
    pub fl_version: Option<String>,
    /// The time base of the file, in ticks per quarter note.
    pub source_ppq: u16,
    /// Every section, in the order of [`ReportSection::ALL`], including
    /// the ones with nothing in them.
    pub categories: Vec<ReportCategory>,
    /// The ids of events in the file that no source this importer is built
    /// on knows. Their data was skipped. A long list means the file comes
    /// from a version of FL Studio newer than the importer.
    pub unknown_event_ids: Vec<u8>,
    /// What went wrong while the file was being read: a cut-off file, a
    /// part that did not add up.
    pub read_problems: Vec<String>,
    /// Every omitted control by device, in the order the devices came up.
    /// Unlike the lines of a category this list is not cut short per
    /// category: it is what the review shows as left out.
    #[serde(default)]
    pub omitted_controls: Vec<OmittedControls>,
    /// Devices with omitted controls beyond [`MAX_OMITTED_DEVICES`].
    #[serde(default)]
    pub more_omitted_devices: u32,
}

impl ImportReport {
    pub(crate) fn new(fl_version: Option<String>, source_ppq: u16) -> Self {
        Self {
            fl_version,
            source_ppq,
            categories: ReportSection::ALL
                .into_iter()
                .map(ReportCategory::new)
                .collect(),
            unknown_event_ids: Vec::new(),
            read_problems: Vec::new(),
            omitted_controls: Vec::new(),
            more_omitted_devices: 0,
        }
    }

    /// Notes that one use of `device` has these decoded fields left out.
    pub(crate) fn omit(&mut self, device: &str, fields: &[&str]) {
        if fields.is_empty() {
            return;
        }
        let device = device.trim();
        if let Some(known) = self
            .omitted_controls
            .iter_mut()
            .find(|known| known.device == device)
        {
            known.instances = known.instances.saturating_add(1);
            for field in fields {
                if !known.fields.iter().any(|known| known == field) {
                    known.fields.push((*field).to_owned());
                }
            }
        } else if self.omitted_controls.len() < MAX_OMITTED_DEVICES {
            self.omitted_controls.push(OmittedControls {
                device: device.to_owned(),
                instances: 1,
                fields: fields.iter().map(|field| (*field).to_owned()).collect(),
            });
        } else {
            self.more_omitted_devices = self.more_omitted_devices.saturating_add(1);
        }
    }

    pub fn category(&self, section: ReportSection) -> &ReportCategory {
        // `new` makes one category for each section, in the order of the
        // enum, and nothing removes any.
        &self.categories[section as usize]
    }

    fn category_mut(&mut self, section: ReportSection) -> &mut ReportCategory {
        &mut self.categories[section as usize]
    }

    /// Counts `count` things of a section as having come across this way.
    pub(crate) fn count(&mut self, section: ReportSection, outcome: Outcome, count: u32) {
        let category = self.category_mut(section);
        let tally = match outcome {
            Outcome::Exact => &mut category.exact,
            Outcome::Approximated => &mut category.approximated,
            Outcome::Placeholder => &mut category.placeholders,
            Outcome::Dropped => &mut category.dropped,
        };
        *tally = tally.saturating_add(count);
    }

    /// Says something about a section. The same words said again raise
    /// the count of the line they are already on.
    pub(crate) fn say(
        &mut self,
        section: ReportSection,
        outcome: Outcome,
        text: impl Into<String>,
    ) {
        self.say_times(section, outcome, 1, text);
    }

    pub(crate) fn say_times(
        &mut self,
        section: ReportSection,
        outcome: Outcome,
        count: u32,
        text: impl Into<String>,
    ) {
        if count == 0 {
            return;
        }
        let text = text.into();
        let category = self.category_mut(section);
        let room = category.lines.len() < MAX_LINES_PER_CATEGORY;
        let same = category
            .lines
            .iter_mut()
            .find(|line| line.outcome == outcome && line.text == text);
        match same {
            Some(line) => line.count = line.count.saturating_add(count),
            None if room => {
                category.lines.push(ReportLine {
                    outcome,
                    count,
                    text,
                });
            }
            None => category.more_lines = category.more_lines.saturating_add(1),
        }
    }

    /// True when nothing was approximated, replaced or dropped, and the
    /// file read without a problem.
    pub fn is_clean(&self) -> bool {
        self.read_problems.is_empty()
            && self.unknown_event_ids.is_empty()
            && self.categories.iter().all(|c| {
                c.approximated == 0 && c.placeholders == 0 && c.dropped == 0 && c.lines.is_empty()
            })
    }

    /// The whole report in one line: for each section with anything in
    /// it, its four counts.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = self
            .categories
            .iter()
            .filter(|category| category.total() > 0)
            .map(|c| {
                format!(
                    "{} {}/{}/{}/{}",
                    c.title.to_lowercase(),
                    c.exact,
                    c.approximated,
                    c.placeholders,
                    c.dropped
                )
            })
            .collect();
        if parts.is_empty() {
            "nothing to import".to_owned()
        } else {
            format!(
                "{} (exact/approximated/placeholder/dropped)",
                parts.join(", ")
            )
        }
    }
}

/// Tracks the outcome of one thing while it is being converted: it starts
/// exact and only ever gets worse.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Worst(pub(crate) Outcome);

impl Worst {
    pub(crate) fn new() -> Self {
        Self(Outcome::Exact)
    }

    pub(crate) fn note(&mut self, outcome: Outcome) {
        self.0 = self.0.max(outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_controls_are_listed_by_device_whatever_the_line_limit() {
        let mut report = ImportReport::new(None, 96);
        // Three hundred uses of one device fill the category far past its
        // line limit; the list of what was left out is not cut with it.
        for index in 0..300 {
            report.omit("Fruity Limiter", &["ceiling", "knee"]);
            report.say(
                ReportSection::Effects,
                Outcome::Dropped,
                format!("Fruity Limiter {index}.ceiling: omitted"),
            );
        }
        report.omit("  Pitcher ", &["speed"]);
        report.omit("Pitcher", &["speed", "formant"]);
        report.omit("Nothing left out", &[]);
        assert!(report.category(ReportSection::Effects).more_lines > 0);
        assert_eq!(
            report.omitted_controls,
            [
                OmittedControls {
                    device: "Fruity Limiter".into(),
                    instances: 300,
                    fields: vec!["ceiling".into(), "knee".into()],
                },
                OmittedControls {
                    device: "Pitcher".into(),
                    instances: 2,
                    fields: vec!["speed".into(), "formant".into()],
                },
            ]
        );
        for device in 0..MAX_OMITTED_DEVICES + 3 {
            report.omit(&format!("Device {device}"), &["x"]);
        }
        assert_eq!(report.omitted_controls.len(), MAX_OMITTED_DEVICES);
        assert_eq!(report.more_omitted_devices, 5);
    }

    #[test]
    fn a_new_report_has_every_section_and_is_clean() {
        let report = ImportReport::new(Some("20.8.4".to_owned()), 96);
        assert_eq!(report.categories.len(), ReportSection::ALL.len());
        for section in ReportSection::ALL {
            assert_eq!(report.category(section).section, section);
            assert_eq!(report.category(section).title, section.title());
        }
        assert!(report.is_clean());
        assert_eq!(report.summary(), "nothing to import");
    }

    #[test]
    fn counts_add_up_by_outcome() {
        let mut report = ImportReport::new(None, 96);
        report.count(ReportSection::Notes, Outcome::Exact, 40);
        report.count(ReportSection::Notes, Outcome::Dropped, 2);
        report.count(ReportSection::Channels, Outcome::Placeholder, 1);
        let notes = report.category(ReportSection::Notes);
        assert_eq!((notes.exact, notes.dropped, notes.total()), (40, 2, 42));
        assert_eq!(
            report.summary(),
            "channels 0/0/1/0, notes 40/0/0/2 (exact/approximated/placeholder/dropped)"
        );
        assert!(!report.is_clean());
    }

    #[test]
    fn the_same_words_twice_are_one_line_with_a_count() {
        let mut report = ImportReport::new(None, 96);
        report.say(
            ReportSection::Mixer,
            Outcome::Approximated,
            "A fader was fitted.",
        );
        report.say(
            ReportSection::Mixer,
            Outcome::Approximated,
            "A fader was fitted.",
        );
        report.say(
            ReportSection::Mixer,
            Outcome::Dropped,
            "A fader was fitted.",
        );
        report.say_times(ReportSection::Mixer, Outcome::Dropped, 0, "Never said.");
        let lines = &report.category(ReportSection::Mixer).lines;
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].outcome, lines[0].count),
            (Outcome::Approximated, 2)
        );
        assert_eq!((lines[1].outcome, lines[1].count), (Outcome::Dropped, 1));
    }

    #[test]
    fn a_category_stops_taking_lines_and_counts_the_rest() {
        let mut report = ImportReport::new(None, 96);
        for index in 0..MAX_LINES_PER_CATEGORY + 7 {
            report.say(
                ReportSection::Effects,
                Outcome::Dropped,
                format!("Effect {index}"),
            );
        }
        let effects = report.category(ReportSection::Effects);
        assert_eq!(effects.lines.len(), MAX_LINES_PER_CATEGORY);
        assert_eq!(effects.more_lines, 7);
    }

    #[test]
    fn an_outcome_only_gets_worse() {
        let mut worst = Worst::new();
        worst.note(Outcome::Approximated);
        worst.note(Outcome::Exact);
        assert_eq!(worst.0, Outcome::Approximated);
        worst.note(Outcome::Dropped);
        worst.note(Outcome::Placeholder);
        assert_eq!(worst.0, Outcome::Dropped);
    }
}
