//! From an [`FlpProject`] to a Windfall [`Project`].
//!
//! The project is built the way the app builds one: by dispatching
//! [`Command`]s to a [`Document`] that starts out empty. Every rule the
//! commands enforce therefore holds for an imported project, and a value
//! FL Studio allows and Windfall does not is brought into range by the
//! same code that does it for a knob.
//!
//! The order is fixed, so the same file always gives the same project,
//! ids included: settings, mixer tracks, their routing, their effects,
//! channels, patterns and notes, the playlist, automation.
//!
//! What cannot come across is left out or stood in for, and the
//! [`ImportReport`] says which. The modules beside this one each convert a
//! part and say in their own documentation what they keep.

mod arrangements;
mod channels;
mod effects;
mod mixer;
mod native;
mod patterns;
mod playlist;
mod slices;
mod synth;
mod timeline;
mod voice;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_dsp::EffectKind;
use windfall_project::{
    ChannelId, Command, Document, EffectId, MAX_TEMPO_BPM, MIN_TEMPO_BPM, PatternId, Project,
    SampleId, SettingsPatch, TimeSignature, TrackId,
};

use crate::error::FlpError;
use crate::model::{FlVersion, FlpProject};
use crate::paths::{Folders, PathStyle};
use crate::plugin::PluginFormat;
use crate::report::{ImportReport, Outcome, ReportSection};

use effects::ParamLink;
pub use slices::{
    NativeSlice, SliceRequest, resolve_layer_samples, resolve_native_slices, resolve_normalization,
    resolve_sample_trim,
};

/// The name of a project whose file has no title, when the caller gives
/// none either.
pub const DEFAULT_PROJECT_NAME: &str = "Imported project";

/// What an import needs to know that the file does not say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ConvertOptions {
    /// The folder the project file is in. Sample paths that are not full
    /// paths are meant from here.
    #[ts(optional)]
    pub project_dir: Option<String>,
    /// The folder FL Studio is installed in on this computer, if it is.
    /// Paths into FL Studio's own content start from here. Without it they
    /// stay as written and the report lists them: Windfall ships none of
    /// that content and never looks for it by itself.
    #[ts(optional)]
    pub factory_data_dir: Option<String>,
    /// FL Studio's user data folder, for paths that start there.
    #[ts(optional)]
    pub user_data_dir: Option<String>,
    /// The user's home folder, for paths that start there.
    #[ts(optional)]
    pub user_profile_dir: Option<String>,
    /// How this computer writes paths.
    pub path_style: PathStyle,
    /// The name for the project when the file has no title: usually the
    /// file's name without its extension.
    #[ts(optional)]
    pub fallback_name: Option<String>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            project_dir: None,
            factory_data_dir: None,
            user_data_dir: None,
            user_profile_dir: None,
            path_style: PathStyle::host(),
            fallback_name: None,
        }
    }
}

/// What an import gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    /// A project that passes [`Project::check`].
    pub project: Project,
    pub report: ImportReport,
    /// The plugins of the FL Studio project that Windfall has nothing for,
    /// with everything the file says about them, for a plugin host to pick
    /// up later.
    pub plugins: Vec<PluginPlaceholder>,
    /// Samplers requesting loop points from the referenced audio file.
    /// The desktop resolves these only after decoding the exact sample.
    pub sampler_loops: Vec<(ChannelId, SampleId)>,
    pub native_slices: Vec<SliceRequest>,
    pub normalization: Vec<(ChannelId, SampleId)>,
    pub sample_trim: Vec<(ChannelId, SampleId, f32)>,
    pub sample_cues: Vec<(ChannelId, SampleId)>,
    pub layer_samples: Vec<(ChannelId, SampleId)>,
}

/// A plugin the Windfall project could not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPlaceholder {
    pub place: PluginPlace,
    /// The name FL Studio knows the plugin by. "Fruity Wrapper" for every
    /// hosted plugin.
    pub internal_name: String,
    /// The plugin's own name, for a hosted plugin whose wrapper could be
    /// read.
    pub name: Option<String>,
    pub vendor: Option<String>,
    /// The plugin standard of a hosted plugin. `None` for one of FL
    /// Studio's own.
    pub format: Option<PluginFormat>,
    /// Where the plugin's file was on the computer that saved the project.
    pub path: Option<String>,
    /// Everything the plugin saved, as FL Studio stored it.
    pub state: Vec<u8>,
}

/// Where in the Windfall project a plugin would go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPlace {
    /// On the silent channel that stands in for an instrument.
    Channel { channel: ChannelId },
    /// In the effect chain of a mixer track, where FL Studio's slot of
    /// this number, from 0, was.
    Effect { track: TrackId, slot: u8 },
}

/// Reads a file and converts it: [`parse`](crate::parse) and then
/// [`convert`].
pub fn import(bytes: &[u8], options: &ConvertOptions) -> Result<Conversion, FlpError> {
    let source = crate::parse(bytes)?;
    if source
        .header
        .as_ref()
        .is_none_or(|h| h.format != crate::FileFormat::Project)
    {
        return Err(FlpError::NotProject);
    }
    Ok(convert(&source, options))
}

/// Builds a Windfall project from what an FL Studio file says.
///
/// It cannot fail: whatever the file holds, the result is a project that
/// passes [`Project::check`], and the report says what became of the rest.
pub fn convert(flp: &FlpProject, options: &ConvertOptions) -> Conversion {
    let mut builder = Builder::new(flp, options);
    builder.settings();
    builder.mixer();
    builder.channels();
    builder.patterns();
    builder.arrangements();
    builder.timeline();
    builder.leftovers();
    builder.finish()
}

/// Colours FL Studio gives things nobody coloured. They are skipped so
/// that such things get a colour from Windfall's palette instead of all
/// being the same grey.
///
/// The first three are the defaults PyFLP documents for patterns and
/// tracks, channels, and inserts; the last two are the insert colours
/// DawVert treats the same way.
const DEFAULT_COLORS: [u32; 5] = [0x48_51_56, 0x5C_65_6A, 0x63_6C_71, 0x87_90_95, 0x78_81_86];

/// A colour worth carrying over: one somebody chose.
fn chosen_color(color: Option<u32>) -> Option<u32> {
    color.filter(|color| !DEFAULT_COLORS.contains(color) && *color <= 0xFF_FFFF)
}

/// An effect of the Windfall project that stands for one of FL Studio's.
struct ImportedEffect {
    track: TrackId,
    id: EffectId,
    kind: EffectKind,
    links: Vec<ParamLink>,
}

/// An audio clip channel, as far as its clips need it.
#[derive(Clone)]
struct AudioSource {
    name: String,
    sample: Option<SampleId>,
    mixer_track: TrackId,
    gain: f32,
    pan: f32,
    reverse: bool,
    pitch: f32,
    /// The channel is time-stretched in FL Studio, which Windfall does not
    /// do.
    stretched: bool,
    muted: bool,
}

/// What a channel of the FL Studio project became.
enum ChannelRole {
    /// A channel of the Windfall project that plays its notes.
    Plays(ChannelId),
    /// A layer: its notes go to these channels of the FL Studio project.
    Layer(Vec<u16>),
    Audio(AudioSource),
    /// An automation clip. Its curve is read when the playlist places it.
    Automation,
}

struct Builder<'a> {
    flp: &'a FlpProject,
    options: &'a ConvertOptions,
    document: Document,
    report: ImportReport,
    plugins: Vec<PluginPlaceholder>,
    sampler_loops: Vec<(ChannelId, SampleId)>,
    native_slices: Vec<SliceRequest>,
    normalization: Vec<(ChannelId, SampleId)>,
    sample_trim: Vec<(ChannelId, SampleId, f32)>,
    sample_cues: Vec<(ChannelId, SampleId)>,
    layer_samples: Vec<(ChannelId, SampleId)>,
    name: String,
    ppq: u16,
    version: Option<FlVersion>,
    /// The tempo the project ended up with.
    tempo_bpm: f64,
    /// Ticks in a bar of the project's time signature.
    bar_ticks: u32,
    /// Mixer tracks by the number of the insert they came from.
    tracks: BTreeMap<u16, TrackId>,
    effects: BTreeMap<(u16, u16), ImportedEffect>,
    channel_filters: BTreeMap<u16, ImportedEffect>,
    /// Sends that exist, by the inserts they connect.
    sends: BTreeSet<(u16, u16)>,
    channels: BTreeMap<u16, ChannelRole>,
    note_fallbacks: BTreeMap<u16, ChannelId>,
    /// Patterns by their FL Studio number, with their length in ticks.
    patterns: BTreeMap<u16, (PatternId, u32)>,
}

impl<'a> Builder<'a> {
    fn new(flp: &'a FlpProject, options: &'a ConvertOptions) -> Self {
        let given = |name: &Option<String>| {
            name.as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        };
        let name = given(&flp.settings.title)
            .or_else(|| given(&options.fallback_name))
            .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_owned());
        let mut report = ImportReport::new(flp.version_text.clone(), flp.ppq());
        report.unknown_event_ids = flp.unknown_ids();
        report.read_problems = flp
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect();
        let project = Project::new(name.clone());
        let bar_ticks = project.settings.time_signature.ticks_per_bar();
        let tempo_bpm = project.settings.tempo_bpm;
        Self {
            flp,
            options,
            document: Document::new(project),
            report,
            plugins: Vec::new(),
            sampler_loops: Vec::new(),
            native_slices: Vec::new(),
            normalization: Vec::new(),
            sample_trim: Vec::new(),
            sample_cues: Vec::new(),
            layer_samples: Vec::new(),
            name,
            ppq: flp.ppq(),
            version: flp.version(),
            tempo_bpm,
            bar_ticks,
            tracks: BTreeMap::from([(0, TrackId::MASTER)]),
            effects: BTreeMap::new(),
            channel_filters: BTreeMap::new(),
            sends: BTreeSet::new(),
            channels: BTreeMap::new(),
            note_fallbacks: BTreeMap::new(),
            patterns: BTreeMap::new(),
        }
    }

    fn project(&self) -> &Project {
        self.document.project()
    }

    fn folders(&self) -> Folders<'a> {
        Folders {
            project: self.options.project_dir.as_deref(),
            factory: self.options.factory_data_dir.as_deref(),
            user_data: self.options.user_data_dir.as_deref(),
            user_profile: self.options.user_profile_dir.as_deref(),
            style: self.options.path_style,
        }
    }

    /// Dispatches a command and returns the ids it made. A command that
    /// fails changes nothing, and the caller says what was lost.
    fn run(&mut self, command: Command) -> Result<Vec<u32>, String> {
        self.document
            .dispatch(command, None)
            .map(|applied| applied.created)
            .map_err(|error| error.to_string())
    }

    /// Dispatches a command that the checks before it should have made
    /// sure of. If it fails all the same, the report says so in the words
    /// of the command's own error, and the import goes on without it.
    fn apply(&mut self, section: ReportSection, what: &str, command: Command) -> Option<Vec<u32>> {
        match self.run(command) {
            Ok(created) => Some(created),
            Err(error) => {
                self.report.say(
                    section,
                    Outcome::Dropped,
                    format!("{what} could not be brought over: {error}."),
                );
                None
            }
        }
    }

    /// The mixer track an insert became. An insert that was not brought
    /// over, and "no insert", play into the master.
    fn track_of(&self, insert: Option<i8>) -> TrackId {
        insert
            .and_then(|insert| u16::try_from(insert).ok())
            .and_then(|insert| self.tracks.get(&insert).copied())
            .unwrap_or(TrackId::MASTER)
    }

    fn settings(&mut self) {
        let section = ReportSection::Project;
        let flp = self.flp;
        let settings = &flp.settings;
        let mut patch = SettingsPatch::default();

        match settings.tempo_bpm() {
            Some(bpm) if (MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&bpm) => {
                patch.tempo_bpm = Some(bpm);
                self.report.count(section, Outcome::Exact, 1);
            }
            Some(bpm) => {
                patch.tempo_bpm = Some(bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM));
                self.report.count(section, Outcome::Approximated, 1);
                self.report.say(
                    section,
                    Outcome::Approximated,
                    format!(
                        "The tempo of {bpm} bpm is outside Windfall's {MIN_TEMPO_BPM} to {MAX_TEMPO_BPM} and was brought to the nearest end."
                    ),
                );
            }
            None => {
                self.report.count(section, Outcome::Approximated, 1);
                self.report.say(
                    section,
                    Outcome::Approximated,
                    "The file gives no tempo, so the project has Windfall's 120 bpm.",
                );
            }
        }

        let numerator = settings.numerator.unwrap_or(4);
        let beat = settings.denominator.unwrap_or(4);
        // Before FL Studio 20 the second number counted the steps in a
        // beat, and the beat was always a quarter note.
        let has_signatures = self.version.is_none_or(|version| version.major >= 20);
        let mut signature = TimeSignature {
            numerator,
            denominator: if has_signatures { beat } else { 4 },
        };
        let mut exact = true;
        if !(1..=16).contains(&numerator) {
            signature.numerator = 4;
            exact = false;
        }
        if ![2, 4, 8, 16].contains(&signature.denominator) {
            signature.denominator = 4;
            exact = false;
        }
        if !exact {
            self.report.say(
                section,
                Outcome::Approximated,
                format!(
                    "The time signature {numerator}/{beat} is not one Windfall has, so the project is in {}/{}.",
                    signature.numerator, signature.denominator
                ),
            );
        } else if !has_signatures && beat != 4 {
            exact = false;
            self.report.say(
                section,
                Outcome::Approximated,
                format!(
                    "The project counts {beat} steps to a beat. Windfall always counts 4, so the step grid reads differently."
                ),
            );
        }
        patch.time_signature = Some(signature);
        if windfall_project::timebase::is_timebase(self.ppq) {
            patch.timebase_ppq = Some(self.ppq);
        }
        let outcome = if exact {
            Outcome::Exact
        } else {
            Outcome::Approximated
        };
        self.report.count(section, outcome, 1);

        if let Some(swing) = settings.swing.filter(|&swing| swing > 0) {
            patch.swing = Some(f32::from(swing.min(128)) / 128.0);
            self.report.count(section, Outcome::Approximated, 1);
            self.report.say(
                section,
                Outcome::Approximated,
                "Swing was carried over as the same share of Windfall's swing, which may not feel the same.",
            );
        }
        // FL Studio's circular law keeps the power of the two sides
        // constant; its triangular law is Windfall's balance law, which is
        // also what a project without the setting has. Only a file that
        // names its law changes anything, and what it names is kept as the
        // nearest law Windfall has rather than counted as a difference.
        match settings.pan_law {
            Some(0) => {
                patch.pan_law = Some(windfall_project::PanLaw::ConstantPower);
                self.report.count(section, Outcome::Exact, 1);
            }
            Some(2) => {
                patch.pan_law = Some(windfall_project::PanLaw::Balance);
                self.report.count(section, Outcome::Exact, 1);
            }
            Some(_) => {
                self.report.count(section, Outcome::Dropped, 1);
                self.report.say(
                    section,
                    Outcome::Dropped,
                    "An unknown panning law was left at Windfall's default.",
                );
            }
            None => {}
        }
        if settings.main_pitch.is_some_and(|pitch| pitch != 0) {
            self.report.count(section, Outcome::Approximated, 1);
            self.report.say(
                section,
                Outcome::Approximated,
                "The project's main pitch is folded into sampler tuning and instrument note expression when the channel's public keyboard.main_pitch flag permits it; stand-in pitch bounds apply.",
            );
        }
        let filled = |text: &Option<String>| text.as_deref().is_some_and(|t| !t.trim().is_empty());
        let bounded = |value: &Option<String>, cap: usize| {
            value.as_deref().map(|text| {
                let text = text.trim();
                let mut end = text.len().min(cap);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text[..end].to_owned()
            })
        };
        patch.author = bounded(&settings.author, 256);
        patch.genre = bounded(&settings.genre, 128);
        patch.comments = bounded(&settings.comments, 16384);
        for (original, mapped) in [
            (&settings.author, &patch.author),
            (&settings.genre, &patch.genre),
            (&settings.comments, &patch.comments),
        ] {
            if original.as_deref().map(str::trim) != mapped.as_deref() {
                self.report.count(section, Outcome::Approximated, 1);
                self.report.say(
                    section,
                    Outcome::Approximated,
                    "Project metadata was brought within Windfall's UTF-8 byte limits.",
                );
            }
        }
        self.report.count(
            section,
            Outcome::Exact,
            [
                (&settings.author, &patch.author),
                (&settings.genre, &patch.genre),
                (&settings.comments, &patch.comments),
            ]
            .into_iter()
            .filter(|(original, mapped)| {
                filled(original) && original.as_deref().map(str::trim) == mapped.as_deref()
            })
            .count() as u32,
        );
        let texts: Vec<&str> = [(filled(&settings.url), "web link")]
            .into_iter()
            .filter_map(|(present, what)| present.then_some(what))
            .collect();
        if !texts.is_empty() {
            self.report
                .count(section, Outcome::Dropped, texts.len() as u32);
            self.report.say(
                section,
                Outcome::Dropped,
                format!(
                    "A Windfall project has no place for these yet, so they were left out: {}.",
                    texts.join(", ")
                ),
            );
        }
        // The name is already the project's.
        self.report.count(
            section,
            if filled(&settings.title) {
                Outcome::Exact
            } else {
                Outcome::Approximated
            },
            1,
        );
        if !filled(&settings.title) {
            self.report.say(section, Outcome::Approximated, "The project had no title; its name comes from the supplied fallback or Imported project.");
        }

        if self
            .apply(
                section,
                "The project settings",
                Command::UpdateSettings { patch },
            )
            .is_some()
        {
            let settings = self.project().settings.clone();
            self.tempo_bpm = settings.tempo_bpm;
            self.bar_ticks = settings.time_signature.ticks_per_bar();
        }
    }

    /// Events the reader could not place. Recorded controls report in patterns.
    fn leftovers(&mut self) {
        let section = ReportSection::Other;
        let flp = self.flp;
        for link in &flp.remote_controllers {
            let source = link.source_channel().and_then(|id| flp.channel(id));
            self.report.say(ReportSection::Automation,if source.is_some_and(|c|c.automation.is_some()){Outcome::Approximated}else{Outcome::Dropped},format!(
                "Controller link source {:#010x} -> {:?}: {}. SCOPE: DawVert auto.py establishes source@2/location@8/flags@12/smoothing@16; PyFLP controller.py leaves source and trailing bytes unnamed. No serialized formula field or smoothing-unit conversion is established; raw flags={} and smoothing={} retained, no formula bytes or milliseconds invented.",
                link.source,link.target(),if source.is_some_and(|c|c.automation.is_some()){"automation clip source mapped"}else{"source is not an imported automation clip; controller/MIDI source encoding lacks a public semantic layout"},link.flags,link.smoothing));
        }
        if flp.uninterpreted_counts.contains_key(&226) {
            self.report.say(section,Outcome::Dropped,"SCOPE: MIDI controller event 226 remains opaque: PyFLP MIDIControllerEvent is only GreedyBytes; DawVert auto.py and LMMS importer establish no MIDI source-field layout or units.");
        }
        for (&id, &count) in &flp.uninterpreted_counts {
            self.report.count(section, Outcome::Dropped, count);
            let what = crate::names::event_name(id).unwrap_or("unknown event");
            self.report.say_times(
                section,
                Outcome::Dropped,
                count,
                format!("Event {id} ({what}) was not interpreted or converted."),
            );
        }
    }

    fn finish(self) -> Conversion {
        let Self {
            document,
            mut report,
            plugins,
            sampler_loops,
            native_slices,
            normalization,
            sample_trim,
            sample_cues,
            layer_samples,
            name,
            flp,
            ..
        } = self;
        let mut project = document.project().clone();
        if !flp.source_event_stream.is_empty() {
            // A valid container lets independent public readers inspect the
            // retained controls with their original owner-selection events.
            let mut data = b"FLhd".to_vec();
            data.extend(6_u32.to_le_bytes());
            data.extend(0_u16.to_le_bytes());
            data.extend(flp.header.map_or(0, |h| h.channel_count).to_le_bytes());
            data.extend(flp.ppq().to_le_bytes());
            data.extend_from_slice(b"FLdt");
            data.extend((flp.source_event_stream.len() as u32).to_le_bytes());
            data.extend_from_slice(&flp.source_event_stream);
            project
                .retained_source_data
                .push(windfall_project::RetainedSourceData {
                    source: "flStudio".into(),
                    format: "flp-events-v1".into(),
                    data,
                });
            report.say(ReportSection::Other, Outcome::Placeholder,
                "Original processor, controller, timing and navigation events are retained opaquely in the saved project. Unsupported controls are not executed; see the conversion warnings. Personal licensee metadata is excluded.");
        }
        project.retained_plugins = plugins
            .iter()
            .map(|plugin| {
                let (channel, track, slot) = match plugin.place {
                    PluginPlace::Channel { channel } => (Some(channel), None, None),
                    PluginPlace::Effect { track, slot } => (None, Some(track), Some(slot)),
                };
                windfall_project::RetainedPluginState {
                    source: "flStudio".to_owned(),
                    internal_name: plugin.internal_name.clone(),
                    name: plugin.name.clone(),
                    vendor: plugin.vendor.clone(),
                    format: plugin.format.map(|format| format.label().to_owned()),
                    path: plugin.path.clone(),
                    channel,
                    track,
                    slot,
                    state: plugin.state.clone(),
                }
            })
            .collect();
        match project.check() {
            Ok(()) => Conversion {
                project,
                report,
                plugins,
                sampler_loops,
                native_slices,
                normalization,
                sample_trim,
                sample_cues,
                layer_samples,
            },
            // The commands keep a project valid, so this cannot happen.
            // If it ever does, an empty project is still better than one
            // the app cannot open.
            Err(problem) => {
                report.read_problems.push(format!(
                    "The converted project did not pass Windfall's own checks ({problem}), so an empty project is given instead. Please report this file."
                ));
                for category in &mut report.categories {
                    category.dropped = category.total();
                    category.exact = 0;
                    category.approximated = 0;
                    category.placeholders = 0;
                }
                let mut empty = Project::new(name);
                empty.retained_plugins = project.retained_plugins;
                empty.retained_source_data = project.retained_source_data;
                Conversion {
                    project: empty,
                    report,
                    plugins,
                    sampler_loops: Vec::new(),
                    native_slices: Vec::new(),
                    normalization: Vec::new(),
                    sample_trim: Vec::new(),
                    sample_cues: Vec::new(),
                    layer_samples: Vec::new(),
                }
            }
        }
    }
}

/// A name for something the file left unnamed or named with blanks.
fn name_or(name: Option<&str>, fallback: impl FnOnce() -> String) -> String {
    match name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => fallback(),
    }
}
