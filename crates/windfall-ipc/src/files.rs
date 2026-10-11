//! Messages for preset, score and template files, and for the saved plugin
//! database. None of them depends on native hosting or on the filesystem.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{
    EffectKind, InstrumentKind, Preset, PresetKind, PresetPluginIdentity, Score,
};

use crate::{PluginEntry, PluginManagerState};

/// A preset taken from the open project, on its way to a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetCapture {
    pub preset: Preset,
    /// The full path of the audio file a sampler preset has to bring
    /// along. Null when the preset has no audio.
    pub audio: Option<String>,
}

/// A preset file as it was read, with what the reader has to say about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetReview {
    /// The file that was read.
    pub path: String,
    pub preset: Preset,
    /// One line for each thing that was changed on the way in, and for each
    /// plugin the preset needs that is not installed.
    pub notes: Vec<String>,
    /// The names of the audio files the preset lists that are not beside it.
    pub missing_audio: Vec<String>,
}

/// One preset of the user's preset library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetEntry {
    pub path: String,
    pub name: String,
    pub kind: PresetKind,
    /// The plugin the preset belongs to. Null for a sampler channel and for
    /// a mixer track.
    pub plugin: Option<PresetPluginIdentity>,
}

/// A score as it was read, with what a conversion left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreReview {
    pub score: Score,
    /// One line for everything a MIDI file held that a score does not, or
    /// the other way round.
    pub notes: Vec<String>,
}

/// A score file that was written, with what the file could not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreWritten {
    /// The file that was written.
    pub path: String,
    /// One line for everything of the score a MIDI file does not hold.
    /// Empty for a score file, which holds all of it.
    pub notes: Vec<String>,
}

/// A project template the user can start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TemplateInfo {
    /// What to ask for the template by. A factory template's id starts with
    /// `factory:`, a user template's with `user:`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Shipped with Windfall: it cannot be replaced or deleted.
    pub factory: bool,
}

/// How far a plugin can be trusted to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PluginStatus {
    /// Part of Windfall.
    Builtin,
    /// The last scan loaded it in the scanner and it can be added.
    Verified,
    /// The last scan could read it but it reported a problem.
    Failed,
    /// It crashed or hung the scanner and is kept from loading until it is
    /// retried.
    Blocked,
    /// It is in the database from an earlier scan and the last scan did not
    /// find it.
    Missing,
}

/// One plugin of the saved database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginRecord {
    /// "builtin", "clap" or "vst3".
    pub format: String,
    /// A hosted plugin's identifier, or a built-in's kind as files spell it.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
    /// Where the plugin's file was last seen. Empty for a built-in.
    pub path: String,
    /// What the plugin is filed under: the user's choice, or the category
    /// worked out from its name.
    pub category: String,
    /// Whether `category` is the user's choice.
    pub categorized: bool,
    pub favorite: bool,
    pub status: PluginStatus,
    /// What the scanner said, for a plugin that failed or is blocked.
    pub detail: Option<String>,
}

/// What the user has said about one plugin. This is all the database file
/// stores beside what the last scan found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PluginAnnotation {
    pub favorite: bool,
    /// Null files the plugin under the category worked out for it.
    pub category: Option<String>,
}

/// The plugin database as the browser and the picker show it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginDatabaseView {
    /// Sorted by category, then name, then format.
    pub records: Vec<PluginRecord>,
    /// Every category a record is filed under, sorted.
    pub categories: Vec<String>,
    /// Why the saved database could not be read or written, if it could not.
    pub error: Option<String>,
}

/// The newest database file this build reads and the one it writes.
pub const PLUGIN_DATABASE_VERSION: u32 = 1;
/// The most plugins the database remembers.
pub const MAX_PLUGIN_RECORDS: usize = 8192;
/// The longest category name, in characters.
pub const MAX_PLUGIN_CATEGORY_CHARS: usize = 48;

/// What a scan found of one hosted plugin, as the database keeps it so that
/// a plugin that goes missing still has its place, favorite and category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownPlugin {
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    pub instrument: bool,
    #[serde(default)]
    pub path: String,
}

/// The saved plugin database: what scans have found, and what the user has
/// said about it. Keys are [`plugin_key`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDatabase {
    pub version: u32,
    #[serde(default)]
    pub known: BTreeMap<String, KnownPlugin>,
    #[serde(default)]
    pub annotations: BTreeMap<String, PluginAnnotation>,
}

impl Default for PluginDatabase {
    fn default() -> Self {
        Self {
            version: PLUGIN_DATABASE_VERSION,
            known: BTreeMap::new(),
            annotations: BTreeMap::new(),
        }
    }
}

/// The key a plugin has in the database: its format and identifier. A
/// plugin keeps it when its file moves.
pub fn plugin_key(format: &str, id: &str) -> String {
    format!("{format}:{id}")
}

/// A category name as the database stores it: trimmed, without control
/// characters, at most [`MAX_PLUGIN_CATEGORY_CHARS`] long. `None` when
/// nothing is left, which files the plugin under its worked-out category.
pub fn normalize_category(category: &str) -> Option<String> {
    // Words first, so that a tab between two words still separates them.
    let cleaned = category
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = cleaned.chars().take(MAX_PLUGIN_CATEGORY_CHARS).collect();
    let cleaned = cleaned.trim().to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// The category a plugin is filed under until the user picks one, worked
/// out from its name. It is a guess from common words and nothing more:
/// a plugin whose name says nothing goes under "Instruments" or "Effects".
pub fn default_category(name: &str, instrument: bool) -> &'static str {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if instrument {
        if has(&["drum", "kick", "snare", "perc", "membrane", "hat", "clap"]) {
            "Drums"
        } else if has(&["sampl", "slice", "grain", "wave ride", "resynth"]) {
            "Samplers"
        } else if has(&["bass", "acid"]) {
            "Bass"
        } else if has(&["pluck", "string", "piano", "keys", "organ"]) {
            "Keys and strings"
        } else {
            "Synths"
        }
    } else if has(&["compress", "limit", "gate", "transient", "maxim", "dynamic"]) {
        "Dynamics"
    } else if has(&["eq", "filter", "shelf", "lowpass", "highpass", "band"]) {
        "EQ and filters"
    } else if has(&["reverb", "room", "space", "convol", "delay", "echo"]) {
        "Delay and reverb"
    } else if has(&["chorus", "flang", "phaser", "tremolo", "vibrato", "lfo"]) {
        "Modulation"
    } else if has(&[
        "distort", "drive", "clip", "satur", "shaper", "lofi", "lo-fi", "crush", "exciter", "amp",
        "fuzz",
    ]) {
        "Distortion"
    } else if has(&["pitch", "vocod", "tune", "shift", "harmon"]) {
        "Pitch and voice"
    } else if has(&[
        "stereo", "spread", "balance", "pan", "polarity", "mute", "dc ", "meter",
    ]) {
        "Utility"
    } else {
        "Effects"
    }
}

impl PluginDatabase {
    /// Reads a database file. A file of a newer version is refused, so that
    /// an older build does not overwrite what it cannot hold.
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Header {
            version: Option<u32>,
        }
        let header: Header = serde_json::from_slice(bytes)
            .map_err(|error| format!("The plugin database is damaged: {error}"))?;
        match header.version {
            Some(version) if version > PLUGIN_DATABASE_VERSION => {
                return Err(format!(
                    "The plugin database was saved by a newer Windfall (version {version})"
                ));
            }
            Some(0) | None => return Err("The plugin database has no version".to_owned()),
            Some(_) => {}
        }
        let mut database: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("The plugin database is damaged: {error}"))?;
        database.tidy();
        Ok(database)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(self).map_err(|error| error.to_string())
    }

    /// Normalizes categories and drops annotations that say nothing.
    fn tidy(&mut self) {
        for annotation in self.annotations.values_mut() {
            annotation.category = annotation.category.as_deref().and_then(normalize_category);
        }
        self.annotations
            .retain(|_, annotation| *annotation != PluginAnnotation::default());
    }

    /// Takes in what a scan found: every plugin it lists is remembered with
    /// its latest name, vendor, kind and file. Plugins of earlier scans
    /// that are not listed stay, so their favorites and categories wait for
    /// them. Returns whether anything changed.
    pub fn absorb(&mut self, state: &PluginManagerState) -> bool {
        let mut changed = false;
        for entry in state.entries.iter().filter(|entry| !entry.id.is_empty()) {
            let key = plugin_key(&entry.format, &entry.id);
            let found = KnownPlugin {
                name: entry.name.clone(),
                vendor: entry.vendor.clone(),
                instrument: entry.instrument,
                path: entry.path.clone(),
            };
            if self.known.get(&key) != Some(&found)
                && (self.known.contains_key(&key) || self.known.len() < MAX_PLUGIN_RECORDS)
            {
                self.known.insert(key, found);
                changed = true;
            }
        }
        changed
    }

    /// Sets what the user says about a plugin. An annotation that says
    /// nothing is removed.
    pub fn annotate(
        &mut self,
        format: &str,
        id: &str,
        annotation: PluginAnnotation,
    ) -> Result<(), String> {
        if !["builtin", "clap", "vst3"].contains(&format) || id.is_empty() || id.len() > 512 {
            return Err("That is not a plugin the database can hold".to_owned());
        }
        let key = plugin_key(format, id);
        let annotation = PluginAnnotation {
            favorite: annotation.favorite,
            category: annotation.category.as_deref().and_then(normalize_category),
        };
        if annotation == PluginAnnotation::default() {
            self.annotations.remove(&key);
        } else if self.annotations.contains_key(&key) || self.annotations.len() < MAX_PLUGIN_RECORDS
        {
            self.annotations.insert(key, annotation);
        } else {
            return Err(format!(
                "The plugin database holds at most {MAX_PLUGIN_RECORDS} annotated plugins"
            ));
        }
        Ok(())
    }

    /// Forgets the plugins of earlier scans that `state` no longer lists
    /// and that the user has said nothing about. Returns how many went.
    pub fn forget_missing(&mut self, state: &PluginManagerState) -> usize {
        let listed: std::collections::HashSet<String> = state
            .entries
            .iter()
            .chain(&state.blocked)
            .map(|entry| plugin_key(&entry.format, &entry.id))
            .collect();
        let before = self.known.len();
        let annotations = &self.annotations;
        self.known
            .retain(|key, _| listed.contains(key) || annotations.contains_key(key));
        before - self.known.len()
    }

    /// The database as it is shown: Windfall's own instruments and effects,
    /// every plugin the last scan found with its verification status, and
    /// the plugins of earlier scans that are missing now.
    pub fn view(&self, state: &PluginManagerState, error: Option<String>) -> PluginDatabaseView {
        let mut records: Vec<PluginRecord> = Vec::new();
        let builtin = |name: &str, id: String, instrument: bool| {
            self.record(
                "builtin",
                &id,
                name,
                "Windfall",
                instrument,
                "",
                PluginStatus::Builtin,
                None,
            )
        };
        for kind in InstrumentKind::ALL {
            if let Some(id) = type_tag(&kind.default_params()) {
                records.push(builtin(kind.name(), id, true));
            }
        }
        for kind in EffectKind::ALL {
            if let Some(id) = type_tag(&kind.default_params()) {
                records.push(builtin(kind.name(), id, false));
            }
        }
        let mut listed = std::collections::HashSet::new();
        let scanned = |entry: &PluginEntry, status: PluginStatus| {
            self.record(
                &entry.format,
                &entry.id,
                &entry.name,
                &entry.vendor,
                entry.instrument,
                &entry.path,
                status,
                entry.error.clone(),
            )
        };
        for entry in &state.entries {
            if entry.id.is_empty() || !listed.insert(plugin_key(&entry.format, &entry.id)) {
                continue;
            }
            let status = if entry.usable {
                PluginStatus::Verified
            } else {
                PluginStatus::Failed
            };
            records.push(scanned(entry, status));
        }
        for entry in &state.blocked {
            // A blocked file that never named a plugin has no identifier;
            // its file is what tells it apart.
            let id = if entry.id.is_empty() {
                &entry.path
            } else {
                &entry.id
            };
            if !listed.insert(plugin_key(&entry.format, id)) {
                continue;
            }
            let mut record = scanned(entry, PluginStatus::Blocked);
            record.id = id.clone();
            records.push(record);
        }
        for (key, known) in &self.known {
            if listed.contains(key) {
                continue;
            }
            let Some((format, id)) = key.split_once(':') else {
                continue;
            };
            records.push(self.record(
                format,
                id,
                &known.name,
                &known.vendor,
                known.instrument,
                &known.path,
                PluginStatus::Missing,
                Some("The last scan did not find this plugin.".to_owned()),
            ));
        }
        records.sort_by(|a, b| {
            (&a.category, a.name.to_lowercase(), &a.format, &a.id).cmp(&(
                &b.category,
                b.name.to_lowercase(),
                &b.format,
                &b.id,
            ))
        });
        let mut categories: Vec<String> = records
            .iter()
            .map(|record| record.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        PluginDatabaseView {
            records,
            categories,
            error,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        format: &str,
        id: &str,
        name: &str,
        vendor: &str,
        instrument: bool,
        path: &str,
        status: PluginStatus,
        detail: Option<String>,
    ) -> PluginRecord {
        let annotation = self.annotations.get(&plugin_key(format, id));
        let chosen = annotation.and_then(|annotation| annotation.category.clone());
        PluginRecord {
            format: format.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            vendor: vendor.to_owned(),
            instrument,
            path: path.to_owned(),
            categorized: chosen.is_some(),
            category: chosen.unwrap_or_else(|| default_category(name, instrument).to_owned()),
            favorite: annotation.is_some_and(|annotation| annotation.favorite),
            status,
            detail,
        }
    }
}

/// The `type` a file gives a built-in processor's settings, which is also
/// what a preset names the processor by.
fn type_tag(params: &impl Serialize) -> Option<String> {
    serde_json::to_value(params)
        .ok()?
        .get("type")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(format: &str, id: &str, name: &str, instrument: bool, usable: bool) -> PluginEntry {
        PluginEntry {
            path: format!("C:/plugins/{name}.{format}"),
            format: format.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            vendor: "Example".to_owned(),
            instrument,
            usable,
            error: (!usable).then(|| "It reported no audio ports.".to_owned()),
        }
    }

    fn scan(entries: Vec<PluginEntry>, blocked: Vec<PluginEntry>) -> PluginManagerState {
        PluginManagerState {
            entries,
            blocked,
            ..PluginManagerState::default()
        }
    }

    fn find<'a>(view: &'a PluginDatabaseView, format: &str, id: &str) -> &'a PluginRecord {
        view.records
            .iter()
            .find(|record| record.format == format && record.id == id)
            .unwrap_or_else(|| panic!("{format}:{id} is not in the view"))
    }

    #[test]
    fn the_view_lists_builtins_and_every_scanned_plugin_with_its_status() {
        let state = scan(
            vec![
                entry("clap", "org.example.synth", "Glass Synth", true, true),
                entry("vst3", "ABCDEF", "Tape Delay", false, true),
                entry("clap", "org.example.broken", "Broken Reverb", false, false),
            ],
            vec![
                entry("clap", "org.example.crash", "Crasher", false, false),
                // A file that crashed before it named a plugin.
                entry("vst3", "", "Blocked file", false, false),
            ],
        );
        let view = PluginDatabase::default().view(&state, None);
        assert_eq!(view.error, None);

        // Every built-in is there once, under the id presets name it by.
        let builtins: Vec<_> = view
            .records
            .iter()
            .filter(|record| record.status == PluginStatus::Builtin)
            .collect();
        assert_eq!(
            builtins.len(),
            InstrumentKind::ALL.len() + EffectKind::ALL.len()
        );
        assert!(builtins.iter().all(|record| record.format == "builtin"
            && record.vendor == "Windfall"
            && record.path.is_empty()
            && !record.id.is_empty()));
        assert_eq!(
            builtins.iter().filter(|record| record.instrument).count(),
            InstrumentKind::ALL.len()
        );
        let limiter = type_tag(&EffectKind::Limiter.default_params()).unwrap();
        assert_eq!(
            find(&view, "builtin", &limiter).name,
            EffectKind::Limiter.name()
        );

        let synth = find(&view, "clap", "org.example.synth");
        assert_eq!(synth.status, PluginStatus::Verified);
        assert!(synth.instrument);
        assert_eq!(synth.category, "Synths");
        assert!(!synth.categorized);
        assert_eq!(synth.detail, None);
        let delay = find(&view, "vst3", "ABCDEF");
        assert_eq!(
            (delay.status, delay.category.as_str()),
            (PluginStatus::Verified, "Delay and reverb")
        );
        let broken = find(&view, "clap", "org.example.broken");
        assert_eq!(broken.status, PluginStatus::Failed);
        assert_eq!(
            broken.detail.as_deref(),
            Some("It reported no audio ports.")
        );
        assert_eq!(
            find(&view, "clap", "org.example.crash").status,
            PluginStatus::Blocked
        );
        let nameless = find(&view, "vst3", "C:/plugins/Blocked file.vst3");
        assert_eq!(nameless.status, PluginStatus::Blocked);

        // Sorted by category and name, with every category listed once.
        let order: Vec<_> = view
            .records
            .iter()
            .map(|record| (record.category.clone(), record.name.to_lowercase()))
            .collect();
        assert!(order.is_sorted());
        let mut categories: Vec<_> = view
            .records
            .iter()
            .map(|record| record.category.clone())
            .collect();
        categories.sort();
        categories.dedup();
        assert_eq!(view.categories, categories);
    }

    #[test]
    fn favorites_and_categories_survive_the_file_and_a_plugin_going_missing() {
        let found = scan(
            vec![
                entry("clap", "org.example.synth", "Glass Synth", true, true),
                entry("vst3", "ABCDEF", "Tape Delay", false, true),
            ],
            Vec::new(),
        );
        let mut database = PluginDatabase::default();
        assert!(database.absorb(&found));
        assert!(!database.absorb(&found), "nothing new the second time");
        database
            .annotate(
                "clap",
                "org.example.synth",
                PluginAnnotation {
                    favorite: true,
                    category: Some("  My\tleads \u{7}  ".to_owned()),
                },
            )
            .unwrap();
        let limiter = type_tag(&EffectKind::Limiter.default_params()).unwrap();
        database
            .annotate(
                "builtin",
                &limiter,
                PluginAnnotation {
                    favorite: true,
                    category: None,
                },
            )
            .unwrap();

        let saved = database.to_json().unwrap();
        let reread = PluginDatabase::from_json(&saved).unwrap();
        assert_eq!(reread, database);

        let view = reread.view(&found, None);
        let synth = find(&view, "clap", "org.example.synth");
        assert!(synth.favorite);
        assert_eq!(synth.category, "My leads");
        assert!(synth.categorized);
        assert!(view.categories.contains(&"My leads".to_owned()));
        assert!(find(&view, "builtin", &limiter).favorite);
        assert!(!find(&view, "vst3", "ABCDEF").favorite);

        // The next scan finds neither hosted plugin: both stay, as missing,
        // and the favorite keeps its place.
        let empty = scan(Vec::new(), Vec::new());
        let view = reread.view(&empty, Some("The drive is not connected.".to_owned()));
        assert_eq!(view.error.as_deref(), Some("The drive is not connected."));
        let synth = find(&view, "clap", "org.example.synth");
        assert_eq!(synth.status, PluginStatus::Missing);
        assert!(synth.favorite);
        assert_eq!(synth.category, "My leads");
        assert_eq!(synth.path, "C:/plugins/Glass Synth.clap");
        assert_eq!(find(&view, "vst3", "ABCDEF").status, PluginStatus::Missing);

        // Forgetting keeps what the user has said something about.
        let mut pruned = reread.clone();
        assert_eq!(pruned.forget_missing(&empty), 1);
        let view = pruned.view(&empty, None);
        assert_eq!(
            find(&view, "clap", "org.example.synth").status,
            PluginStatus::Missing
        );
        assert!(
            !view
                .records
                .iter()
                .any(|record| record.format == "vst3" && record.id == "ABCDEF")
        );

        // Clearing an annotation removes it from the file.
        pruned
            .annotate("clap", "org.example.synth", PluginAnnotation::default())
            .unwrap();
        assert!(
            !pruned
                .annotations
                .contains_key(&plugin_key("clap", "org.example.synth"))
        );
        assert_eq!(pruned.forget_missing(&empty), 1);
    }

    #[test]
    fn damaged_newer_and_invalid_input_is_refused() {
        assert!(PluginDatabase::from_json(b"not json").is_err());
        assert!(
            PluginDatabase::from_json(b"{}")
                .unwrap_err()
                .contains("no version")
        );
        let newer = format!("{{\"version\":{}}}", PLUGIN_DATABASE_VERSION + 1);
        assert!(
            PluginDatabase::from_json(newer.as_bytes())
                .unwrap_err()
                .contains("newer Windfall")
        );
        let mut database = PluginDatabase::default();
        for (format, id) in [("au", "x"), ("clap", ""), ("", "x")] {
            assert!(
                database
                    .annotate(
                        format,
                        id,
                        PluginAnnotation {
                            favorite: true,
                            category: None
                        }
                    )
                    .is_err(),
                "{format}:{id}"
            );
        }
        assert!(database.annotations.is_empty());

        assert_eq!(normalize_category("   "), None);
        assert_eq!(normalize_category("\u{1}\u{2}"), None);
        assert_eq!(
            normalize_category(&"x".repeat(200))
                .unwrap()
                .chars()
                .count(),
            MAX_PLUGIN_CATEGORY_CHARS
        );
        assert_eq!(
            normalize_category(" Drum   bus ").as_deref(),
            Some("Drum bus")
        );
        // A hand-edited file is tidied on the way in.
        let edited = br#"{"version":1,"annotations":{"clap:a":{"favorite":false,"category":"  "},"clap:b":{"category":" Keys "}}}"#;
        let database = PluginDatabase::from_json(edited).unwrap();
        assert_eq!(database.annotations.len(), 1);
        assert_eq!(
            database.annotations["clap:b"].category.as_deref(),
            Some("Keys")
        );
    }

    #[test]
    fn names_that_say_what_a_plugin_is_file_it_and_others_fall_back() {
        for (name, instrument, category) in [
            ("Kick", true, "Drums"),
            ("Grain Cloud", true, "Samplers"),
            ("Acid Line", true, "Bass"),
            ("Pluck", true, "Keys and strings"),
            ("Matrix FM", true, "Synths"),
            ("Compressor", false, "Dynamics"),
            ("Parametric EQ", false, "EQ and filters"),
            ("Reverb", false, "Delay and reverb"),
            ("Chorus", false, "Modulation"),
            ("Overdrive", false, "Distortion"),
            ("Pitch Shift", false, "Pitch and voice"),
            ("Stereo Enhancer", false, "Utility"),
            ("Mystery Box", false, "Effects"),
        ] {
            assert_eq!(default_category(name, instrument), category, "{name}");
        }
    }
}
