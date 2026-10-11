//! Only the selected arrangement's documented named/meter markers cross over.
use super::Builder;
use crate::report::{Outcome, ReportSection};
use windfall_project::{
    Command, MAX_PATTERN_TICKS, MarkerKind, PatternId, PatternTimelineEdit, TimeSignature,
};

impl Builder<'_> {
    pub(super) fn pattern_timeline(
        &mut self,
        pattern: PatternId,
        markers: &[crate::model::TimeMarker],
    ) {
        let section = ReportSection::Other;
        for marker in markers {
            let scaled = crate::units::rescale(marker.position, self.ppq);
            let Ok(tick) = u32::try_from(scaled.ticks) else {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    "A pattern marker exceeds the pattern tick range.",
                );
                continue;
            };
            if tick > MAX_PATTERN_TICKS {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    "A pattern marker exceeds the pattern tick range.",
                );
                continue;
            }
            let edit = match marker.kind {
                8 => {
                    let (Some(numerator), Some(denominator)) =
                        (marker.numerator, marker.denominator)
                    else {
                        self.report.count(section, Outcome::Dropped, 1);
                        self.report.say(
                            section,
                            Outcome::Dropped,
                            "A pattern meter marker has no complete signature.",
                        );
                        continue;
                    };
                    PatternTimelineEdit::AddMeter {
                        tick,
                        signature: TimeSignature {
                            numerator,
                            denominator,
                        },
                    }
                }
                0 => PatternTimelineEdit::AddMarker {
                    tick,
                    name: marker
                        .name
                        .as_deref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("Marker")
                        .to_owned(),
                },
                kind => {
                    self.report.count(section, Outcome::Dropped, 1);
                    self.report.say(section, Outcome::Dropped, format!("Pattern marker kind {kind} is not mapped: its navigation semantics are not supported by this importer."));
                    continue;
                }
            };
            let Some(source) = self.project().pattern(pattern) else {
                continue;
            };
            let command = Command::EditPatternTimeline {
                pattern,
                expected: source.timeline.clone(),
                expected_signature: source.time_signature,
                edit,
            };
            if self
                .apply(section, "A pattern timeline marker", command)
                .is_some()
            {
                self.report.count(
                    section,
                    if scaled.exact {
                        Outcome::Exact
                    } else {
                        Outcome::Approximated
                    },
                    1,
                );
            } else {
                self.report.count(section, Outcome::Dropped, 1);
            }
        }
    }

    pub(super) fn timeline(&mut self) {
        let section = ReportSection::Other;
        let markers = self
            .flp
            .main_arrangement()
            .map(|a| a.markers.clone())
            .unwrap_or_default();
        for marker in markers {
            let scaled = crate::units::rescale(marker.position, self.ppq);
            let Ok(tick) = u32::try_from(scaled.ticks) else {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    "An arrangement marker exceeds the song tick range.",
                );
                continue;
            };
            let command = match marker.kind {
                8 => {
                    let (Some(numerator), Some(denominator)) =
                        (marker.numerator, marker.denominator)
                    else {
                        self.report.count(section, Outcome::Dropped, 1);
                        self.report.say(
                            section,
                            Outcome::Dropped,
                            "An arrangement meter marker has no complete signature.",
                        );
                        continue;
                    };
                    Command::AddMeterChange {
                        tick,
                        signature: TimeSignature {
                            numerator,
                            denominator,
                        },
                    }
                }
                0 => Command::AddTimelineMarker {
                    tick,
                    name: marker
                        .name
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| "Marker".into()),
                    kind: MarkerKind::Named,
                },
                kind => {
                    self.report.count(section, Outcome::Dropped, 1);
                    self.report.say(section, Outcome::Dropped, format!("Arrangement marker kind {kind} is not mapped: its navigation semantics are not supported by this importer."));
                    continue;
                }
            };
            if self
                .apply(section, "An arrangement timeline marker", command)
                .is_some()
            {
                self.report.count(
                    section,
                    if scaled.exact {
                        Outcome::Exact
                    } else {
                        Outcome::Approximated
                    },
                    1,
                );
            } else {
                self.report.count(section, Outcome::Dropped, 1);
            }
        }
    }
}
