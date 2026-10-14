//! Independent clip, track and supported musical-timeline layouts.
use super::Builder;
use crate::report::{Outcome, ReportSection};
use std::collections::BTreeMap;
use windfall_project::{ClipId, Command, PlaylistTrackId};

pub(super) struct PlaylistReferences {
    pub tracks: BTreeMap<u16, PlaylistTrackId>,
    pub items: Vec<Vec<ClipId>>,
    pub legacy_items: Vec<Vec<ClipId>>,
}

fn arrangement_name(source: Option<&str>, index: u16) -> String {
    let mut name = source.unwrap_or_default().replace('\0', "");
    let mut end = name.len().min(256);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    if name.trim().is_empty() {
        format!("Arrangement {}", u32::from(index) + 1)
    } else {
        name
    }
}

impl Builder<'_> {
    pub(super) fn arrangements(&mut self) {
        let flp = self.flp;
        let Some(selected) = flp.main_arrangement() else {
            return;
        };
        if flp.arrangements.len() <= 1 {
            self.playlist_arrangement(selected);
            return;
        }
        let section = ReportSection::Other;
        let mut selected_id = None;
        for (index, source) in flp.arrangements.iter().enumerate() {
            let name = arrangement_name(source.name.as_deref(), source.index);
            let changed = source.name.as_deref() != Some(name.as_str());
            let command = if index == 0 {
                Command::AddArrangement {
                    name,
                    clips: Vec::new(),
                    tracks: Vec::new(),
                }
            } else {
                Command::AddArrangementLayout { name, copy: false }
            };
            let Some(id) = self.create(section, "An arrangement layout", command) else {
                self.report.count(section, Outcome::Dropped, 1);
                continue;
            };
            self.apply(
                section,
                &windfall_core::notice!("flp.subject.importedArrangement").to_string(),
                Command::SwitchArrangement { id },
            );
            let references = self.playlist_arrangement(source);
            // AddArrangementLayout inherits tracks from its source; replace those
            // references with this arrangement's own lanes after conversion.
            self.apply(
                section,
                &windfall_core::notice!("flp.subject.arrangementLayout").to_string(),
                Command::SetArrangementReferences {
                    id,
                    clips: references
                        .items
                        .into_iter()
                        .chain(references.legacy_items)
                        .flatten()
                        .collect(),
                    tracks: references.tracks.into_values().collect(),
                },
            );
            self.report.count(
                section,
                if changed {
                    Outcome::Approximated
                } else {
                    Outcome::Exact
                },
                1,
            );
            self.arrangement_timeline(&source.markers);
            self.apply(
                section,
                &windfall_core::notice!("flp.subject.musicalTimeline").to_string(),
                Command::CaptureArrangementTimeline { id },
            );
            if std::ptr::eq(source, selected) {
                selected_id = Some(id);
            }
        }
        if let Some(id) = selected_id {
            self.apply(
                section,
                &windfall_core::notice!("flp.subject.selectedArrangement").to_string(),
                Command::SwitchArrangement { id },
            );
        }
    }
}

#[cfg(test)]
mod tests;
