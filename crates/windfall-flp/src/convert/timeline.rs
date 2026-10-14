//! Documented named/meter markers on independent arrangement and pattern timelines.
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
                    windfall_core::notice!("flp.timeline.patternMarkerRange").to_string(),
                );
                continue;
            };
            if tick > MAX_PATTERN_TICKS {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    windfall_core::notice!("flp.timeline.patternMarkerRange").to_string(),
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
                            windfall_core::notice!("flp.timeline.patternMeterIncomplete")
                                .to_string(),
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
                    self.report.say(
                        section,
                        Outcome::Dropped,
                        windfall_core::notice!("flp.timeline.patternMarkerKind", kind = kind)
                            .to_string(),
                    );
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
                .apply(
                    section,
                    &windfall_core::notice!("flp.subject.patternTimelineMarker").to_string(),
                    command,
                )
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
        if !self
            .project()
            .playlist
            .arrangement_book
            .timelines
            .is_empty()
        {
            return;
        }
        let markers = self
            .flp
            .main_arrangement()
            .map(|a| a.markers.clone())
            .unwrap_or_default();
        self.arrangement_timeline(&markers);
    }

    pub(super) fn arrangement_timeline(&mut self, markers: &[crate::model::TimeMarker]) {
        let section = ReportSection::Other;
        // DawVert names actions 1..5, 9,10. The public Playlist manual says
        // marker-loop/skip end at the next marker and song-loop ends at data.
        // Endpoints come from the source timeline, never from arbitrary bars.
        for marker in markers {
            let scaled = crate::units::rescale(marker.position, self.ppq);
            let Ok(tick) = u32::try_from(scaled.ticks) else {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    windfall_core::notice!("flp.timeline.arrangementMarkerRange").to_string(),
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
                            windfall_core::notice!("flp.timeline.arrangementMeterIncomplete")
                                .to_string(),
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
                        .as_deref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("Marker")
                        .to_owned(),
                    kind: MarkerKind::Named,
                },
                1 | 2 | 4 => {
                    let source_end = if marker.kind == 4 {
                        self.project()
                            .playlist
                            .clips
                            .iter()
                            .filter(|clip| self.project().playlist.arrangement_book.shows(clip))
                            .map(|c| c.start.saturating_add(c.length))
                            .max()
                            .unwrap_or(0)
                            .max(tick)
                    } else {
                        markers
                            .iter()
                            .filter(|m| m.position > marker.position)
                            .map(|m| crate::units::rescale(m.position, self.ppq).ticks)
                            .min()
                            .and_then(|t| u32::try_from(t).ok())
                            .unwrap_or(tick)
                    };
                    if source_end <= tick {
                        self.report.count(section, Outcome::Dropped, 1);
                        self.report.say(
                            section,
                            Outcome::Dropped,
                            windfall_core::notice!(
                                "flp.timeline.noEndpoint",
                                name = format!("{:?}", marker.name),
                                kind = marker.kind
                            )
                            .to_string(),
                        );
                        continue;
                    }
                    Command::AddTimelineMarker {
                        tick,
                        name: marker
                            .name
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| "Imported region".into()),
                        kind: if marker.kind == 2 {
                            MarkerKind::Skip { end: source_end }
                        } else {
                            MarkerKind::Loop { end: source_end }
                        },
                    }
                }
                3 => {
                    let previous = markers
                        .iter()
                        .filter(|m| m.position < marker.position)
                        .max_by_key(|m| m.position);
                    let suppressed = previous.is_some_and(|m| m.kind == 4);
                    if suppressed {
                        self.report.say(
                            section,
                            Outcome::Exact,
                            windfall_core::notice!(
                                "flp.timeline.pauseSuppressed",
                                name = format!("{:?}", marker.name)
                            )
                            .to_string(),
                        );
                    }
                    Command::AddTimelineMarker {
                        tick,
                        name: marker
                            .name
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| "Pause".into()),
                        kind: if suppressed {
                            MarkerKind::Named
                        } else {
                            MarkerKind::Pause
                        },
                    }
                }
                5 | 9 | 10 => {
                    let kind = if marker.kind == 5 {
                        MarkerKind::Start
                    } else if marker.kind == 9 {
                        let end = markers
                            .iter()
                            .filter(|m| m.position > marker.position && m.kind == 10)
                            .min_by_key(|m| m.position)
                            .map(|m| crate::units::rescale(m.position, self.ppq).ticks)
                            .and_then(|t| u32::try_from(t).ok());
                        match end.filter(|end| *end > tick) {
                            Some(end) => MarkerKind::Punch { end },
                            None => {
                                self.report.say(
                                    section,
                                    Outcome::Dropped,
                                    windfall_core::notice!(
                                        "flp.timeline.punchWithoutEnd",
                                        name = format!("{:?}", marker.name)
                                    )
                                    .to_string(),
                                );
                                MarkerKind::Named
                            }
                        }
                    } else {
                        MarkerKind::Named
                    };
                    self.report.say(
                        section,
                        Outcome::Approximated,
                        windfall_core::notice!(
                            "flp.timeline.markerTranslated",
                            name = format!("{:?}", marker.name),
                            kind = marker.kind
                        )
                        .to_string(),
                    );
                    Command::AddTimelineMarker {
                        tick,
                        name: marker
                            .name
                            .clone()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| {
                                match marker.kind {
                                    5 => "Start",
                                    9 => "Punch in",
                                    _ => "Punch out",
                                }
                                .into()
                            }),
                        kind,
                    }
                }
                kind => {
                    self.report.count(section, Outcome::Dropped, 1);
                    self.report.say(
                        section,
                        Outcome::Dropped,
                        windfall_core::notice!("flp.timeline.arrangementMarkerScope", kind = kind)
                            .to_string(),
                    );
                    continue;
                }
            };
            if self
                .apply(
                    section,
                    &windfall_core::notice!("flp.subject.arrangementTimelineMarker").to_string(),
                    command,
                )
                .is_some()
            {
                self.report.say(
                    section,
                    if scaled.exact && !matches!(marker.kind, 4 | 5 | 9 | 10) {
                        Outcome::Exact
                    } else {
                        Outcome::Approximated
                    },
                    windfall_core::notice!(
                        "flp.timeline.arrangementMarker",
                        kind = marker.kind,
                        name = format!("{:?}", marker.name),
                        tick = tick
                    )
                    .to_string(),
                );
                self.report.count(
                    section,
                    if scaled.exact && !matches!(marker.kind, 4 | 5 | 9 | 10) {
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
