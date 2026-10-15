//! The edit tools. Each one reads the project, checks its arguments
//! against it, builds the [`Command`]s that make the change, tries them on
//! a copy of the project for the preview, and files them as one
//! [`CommandProposal`]. None of them changes the project: the user, or an
//! agent the user allowed, applies the proposal later as one undo step.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use windfall_dsp::{ParamInfo, ParamKind};
use windfall_project::proposal::CommandProposal;
use windfall_project::{
    AutomationId, AutomationPoint, AutomationTarget, ChannelId, ChannelPatch, ClipContent, ClipId,
    ClipInit, ClipPatch, ClipUpdate, Command, EffectId, EffectKind, EffectSlotPatch,
    InstrumentKind, MAX_GAIN, MAX_PATTERN_STEPS, MAX_SONG_TICKS, MAX_TEMPO_BPM, MIN_TEMPO_BPM,
    MixerTrackPatch, Note, NoteId, NoteInit, NotePatch, NoteUpdate, PatternId, PatternPatch,
    PlaylistTrackId, PlaylistTrackPatch, Project, SettingsPatch, TICKS_PER_STEP, TrackId,
};

use crate::approvals::{Approver, Preview, Status};
use crate::args::{self, Args};
use crate::host::{NativeEdit, Snapshot};
use crate::music::{self, Grid};
use crate::summary::{self, ParamTarget, kind_name};
use crate::tools::Call;

/// Arguments every edit tool takes besides its own.
pub const COMMON: [&str; 2] = ["title", "explanation"];
const MAX_NOTES: usize = 512;
const MAX_CLIPS: usize = 128;
const MAX_POINTS: usize = 1024;

/// What a tool wants done, before it is a proposal.
pub struct Draft {
    /// The undo step's name when the agent gives none, as in "Add notes".
    pub label: &'static str,
    pub commands: Vec<Command>,
    /// One line per change, for the agent.
    pub changes: Vec<String>,
    /// Ids the commands will create, worked out from the project's id
    /// allocator. The preview checks each against what the copy made.
    pub expected: Vec<u32>,
}

impl Draft {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            commands: Vec::new(),
            changes: Vec::new(),
            expected: Vec::new(),
        }
    }
}

/// The project's id allocator, followed as commands are added. Every
/// command that creates something takes the next ids in the order its
/// documentation gives.
struct Ids(u32);

impl Ids {
    fn of(project: &Project) -> Self {
        Self(project.next_id)
    }
    fn take(&mut self) -> u32 {
        let id = self.0;
        self.0 += 1;
        id
    }
}

fn allowed<'a>(own: &[&'a str]) -> Vec<&'a str> {
    own.iter().copied().chain(COMMON).collect()
}

/// The undo step's name: the agent's name, then what it does.
pub fn history_label(agent: &str, title: &str) -> String {
    let mut label = format!("{agent}: {title}");
    if label.len() > 128 {
        let mut end = 128;
        while !label.is_char_boundary(end) {
            end -= 1;
        }
        label.truncate(end);
    }
    label
}

/// Tries a proposal on a copy of the project: the same checks the apply
/// makes, and what it would create and change.
pub fn preview(
    snapshot: &Snapshot,
    proposal: &CommandProposal,
    changes: Vec<String>,
    expected: &[u32],
) -> Result<Preview, String> {
    let batch = proposal.validate(&snapshot.document, snapshot.stamp, snapshot.recording)?;
    let mut candidate = snapshot.document.clone();
    let applied = candidate
        .dispatch(batch, None)
        .map_err(|error| error.to_string())?;
    if expected.iter().any(|id| !applied.created.contains(id)) {
        return Err("the preview created other ids than the tool expected; read the project again and retry".into());
    }
    let before = snapshot.document.project();
    let after = candidate.project();
    Ok(Preview {
        changes,
        created: created(before, after),
        counts: counts(before, after),
    })
}

fn new_ids<T: Ord + Copy>(
    before: impl Iterator<Item = T>,
    after: impl Iterator<Item = T>,
) -> Vec<T> {
    let old: BTreeSet<T> = before.collect();
    // Ids are allocated in order, so sorted is the order they were made in.
    let made: BTreeSet<T> = after.filter(|id| !old.contains(id)).collect();
    made.into_iter().collect()
}

fn effect_ids(project: &Project) -> impl Iterator<Item = u32> + '_ {
    project
        .mixer
        .tracks
        .iter()
        .flat_map(|track| track.effects.iter().map(|effect| effect.id.0))
}

fn note_ids(project: &Project) -> impl Iterator<Item = u32> + '_ {
    project.patterns.iter().flat_map(|pattern| {
        pattern
            .lanes
            .iter()
            .flat_map(|lane| lane.notes.iter().map(|note| note.id.0))
    })
}

fn created(before: &Project, after: &Project) -> Value {
    let mut created = serde_json::Map::new();
    let mut put = |kind: &str, ids: Vec<u32>| {
        if !ids.is_empty() {
            created.insert(kind.to_owned(), json!(ids));
        }
    };
    put(
        "channels",
        new_ids(
            before.channels.iter().map(|c| c.id.0),
            after.channels.iter().map(|c| c.id.0),
        ),
    );
    put(
        "patterns",
        new_ids(
            before.patterns.iter().map(|p| p.id.0),
            after.patterns.iter().map(|p| p.id.0),
        ),
    );
    put(
        "mixerTracks",
        new_ids(
            before.mixer.tracks.iter().map(|t| t.id.0),
            after.mixer.tracks.iter().map(|t| t.id.0),
        ),
    );
    put("effects", new_ids(effect_ids(before), effect_ids(after)));
    put(
        "playlistTracks",
        new_ids(
            before.playlist.tracks.iter().map(|t| t.id.0),
            after.playlist.tracks.iter().map(|t| t.id.0),
        ),
    );
    put(
        "clips",
        new_ids(
            before.playlist.clips.iter().map(|c| c.id.0),
            after.playlist.clips.iter().map(|c| c.id.0),
        ),
    );
    put(
        "automations",
        new_ids(
            before.automations.iter().map(|a| a.id.0),
            after.automations.iter().map(|a| a.id.0),
        ),
    );
    let mut notes = new_ids(note_ids(before), note_ids(after));
    notes.truncate(MAX_NOTES);
    put("notes", notes);
    Value::Object(created)
}

/// How many of each kind of thing the project has before and after, for
/// the kinds that change, and the tempo and meter when they do. The panel
/// says these in the interface's language.
fn counts(before: &Project, after: &Project) -> BTreeMap<String, [Value; 2]> {
    let tally = |project: &Project| -> [(&'static str, usize); 8] {
        [
            ("channels", project.channels.len()),
            ("patterns", project.patterns.len()),
            ("notes", note_ids(project).count()),
            ("mixerTracks", project.mixer.tracks.len()),
            ("effects", effect_ids(project).count()),
            ("playlistTracks", project.playlist.tracks.len()),
            ("clips", project.playlist.clips.len()),
            ("automations", project.automations.len()),
        ]
    };
    let mut counts = BTreeMap::new();
    for ((kind, old), (_, new)) in tally(before).into_iter().zip(tally(after)) {
        if old != new {
            counts.insert(kind.to_owned(), [json!(old), json!(new)]);
        }
    }
    if before.settings.tempo_bpm != after.settings.tempo_bpm {
        counts.insert(
            "tempoBpm".into(),
            [
                json!(before.settings.tempo_bpm),
                json!(after.settings.tempo_bpm),
            ],
        );
    }
    if before.settings.time_signature != after.settings.time_signature {
        counts.insert(
            "meter".into(),
            [
                json!(music::format_meter(before.settings.time_signature)),
                json!(music::format_meter(after.settings.time_signature)),
            ],
        );
    }
    counts
}

/// Files the draft as a proposal, and applies it at once when the user
/// lets this agent apply without asking.
pub fn propose(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
    draft: Draft,
) -> Result<Value, String> {
    propose_native(call, snapshot, args, draft, None)
}

/// Tries a proposal that holds an edit Windfall makes itself on a copy of
/// the project. A SoundFont load is tried with the commands it will be
/// applied as, without reading the file; a channel preset, which is read
/// only when applied, adds nothing to the preview.
pub fn native_preview(
    snapshot: &Snapshot,
    proposal: &CommandProposal,
    native: &NativeEdit,
    changes: Vec<String>,
    expected: &[u32],
) -> Result<Preview, String> {
    if snapshot.recording {
        return Err("proposals cannot apply while recording".into());
    }
    if proposal.label.trim().is_empty() || proposal.label.len() > 128 {
        return Err("proposal label/explanation limit exceeded".into());
    }
    let mut candidate = snapshot.document.clone();
    let mut created_ids = Vec::new();
    if !proposal.commands.is_empty() {
        let applied = candidate
            .dispatch(proposal.batch()?, None)
            .map_err(|error| error.to_string())?;
        created_ids = applied.created;
    }
    if let NativeEdit::SoundFont {
        channel,
        path,
        bank,
        program,
        replace_instrument,
        ..
    } = native
    {
        let stored = windfall_project::SamplePath::External(path.clone());
        let existing = candidate
            .project()
            .samples
            .iter()
            .find(|sample| sample.path == stored)
            .map(|sample| sample.id);
        let (commands, _) = windfall_project::sample_instrument::soundfont_load_commands(
            &candidate,
            *channel,
            file_stem(path),
            stored,
            existing,
            *bank,
            *program,
            *replace_instrument,
        )?;
        candidate
            .dispatch(
                Command::Batch {
                    label: Some(proposal.label.clone()),
                    commands,
                },
                None,
            )
            .map_err(|error| error.to_string())?;
        candidate.project().check()?;
    }
    if expected.iter().any(|id| !created_ids.contains(id)) {
        return Err("the preview created other ids than the tool expected; read the project again and retry".into());
    }
    let before = snapshot.document.project();
    let after = candidate.project();
    Ok(Preview {
        changes,
        created: created(before, after),
        counts: counts(before, after),
    })
}

/// A file's name without its folder and extension.
pub fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name.to_owned(),
    }
}

/// Files the draft, with an edit Windfall makes itself when there is one.
pub fn propose_native(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
    draft: Draft,
    native: Option<NativeEdit>,
) -> Result<Value, String> {
    if draft.commands.is_empty() && native.is_none() {
        return Err("nothing would change, so no proposal was made".into());
    }
    let title = args::optional_text(args, "title", 80)?
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(draft.label);
    let explanation = args::optional_text(args, "explanation", 2000)?.unwrap_or("");
    let proposal = CommandProposal {
        stamp: snapshot.stamp,
        label: history_label(call.agent, title),
        explanation: explanation.to_owned(),
        commands: draft.commands,
    };
    let preview = match &native {
        None => preview(snapshot, &proposal, draft.changes, &draft.expected)?,
        Some(edit) => native_preview(snapshot, &proposal, edit, draft.changes, &draft.expected)?,
    };
    let label = proposal.label.clone();
    let id = call
        .approvals
        .submit_native(call.agent, proposal, native, preview.clone())?;
    call.host.proposals_changed();
    let status = if call.approvals.trusted(call.agent) {
        call.approvals
            .apply(call.host, &id, Approver::Agent(call.agent))?
    } else {
        Status::Pending
    };
    let next = match &status {
        Status::Pending => {
            "The user reviews this in Windfall. Call get_proposal with waitSeconds to wait for the decision."
        }
        Status::Applied { .. } => "Applied as one undo step.",
        _ => "Call get_proposal for details.",
    };
    Ok(json!({
        "proposalId": id,
        "status": status,
        "label": label,
        "changes": preview.changes,
        "created": preview.created,
        "counts": preview.counts,
        "stamp": snapshot.stamp,
        "next": next,
    }))
}

/// A SoundFont preset an agent named, checked against the file: the file
/// inside the browser's folders, the preset in it.
pub struct SoundFontChoice {
    pub path: String,
    pub preset: windfall_dsp::sample_bank::soundfont::SoundFontPreset,
}

/// Reads `{path, bank, program}` and checks them with Windfall.
pub fn soundfont_choice(call: &Call<'_>, args: &Args) -> Result<SoundFontChoice, String> {
    let path = call.host.library_file(args::text(args, "path", 4096)?)?;
    let bank = args::optional_u32(args, "bank", 0..=u32::from(u16::MAX))?.unwrap_or(0);
    let program = args::optional_u32(args, "program", 0..=u32::from(u16::MAX))?
        .ok_or("argument program is required")?;
    let presets = call.host.soundfont_presets(&path)?;
    let preset = presets
        .into_iter()
        .find(|preset| u32::from(preset.bank) == bank && u32::from(preset.program) == program)
        .ok_or_else(|| {
            format!("the SoundFont has no preset at bank {bank}, program {program}; list_soundfont_presets lists its presets")
        })?;
    Ok(SoundFontChoice { path, preset })
}

/// `load_soundfont_preset`.
pub fn load_soundfont_preset(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["channelId", "path", "bank", "program", "replaceInstrument"]),
    )?;
    let project = snapshot.document.project();
    let channel = summary::channel_of(project, args::id(args, "channelId")?)?;
    let replace = args::optional_bool(args, "replaceInstrument")?.unwrap_or(false);
    let kind = match &channel.source {
        windfall_project::ChannelSource::Instrument { params, .. } => params.kind(),
        windfall_project::ChannelSource::Sampler(_) => {
            return Err(format!(
                "channel {} \"{}\" is a sampler, which cannot play a SoundFont; add_channel with soundFont adds a SoundFont channel",
                channel.id.0, channel.name
            ));
        }
    };
    if kind != InstrumentKind::SoundFont && !replace {
        return Err(format!(
            "channel {} \"{}\" plays {}, not a SoundFont player. Set replaceInstrument to true to make it a SoundFont player (its notes, mixer track and name stay), or add a channel with add_channel and soundFont",
            channel.id.0,
            channel.name,
            kind.name()
        ));
    }
    let choice = soundfont_choice(call, args)?;
    let mut draft = Draft::new("Load SoundFont preset");
    if kind != InstrumentKind::SoundFont {
        draft.changes.push(format!(
            "make channel {} a SoundFont player instead of {}",
            channel.id.0,
            kind.name()
        ));
    }
    draft.changes.push(format!(
        "load SoundFont preset \"{}\" (bank {}, program {}) from {} into channel {}",
        choice.preset.name, choice.preset.bank, choice.preset.program, choice.path, channel.id.0
    ));
    let native = NativeEdit::SoundFont {
        channel: channel.id,
        path: choice.path,
        bank: choice.preset.bank,
        program: choice.preset.program,
        preset: choice.preset.name,
        replace_instrument: kind != InstrumentKind::SoundFont,
    };
    propose_native(call, snapshot, args, draft, Some(native))
}

/// `apply_channel_preset`.
pub fn apply_channel_preset(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
) -> Result<Value, String> {
    args::known(args, &allowed(&["channelId", "preset"]))?;
    let project = snapshot.document.project();
    let channel = summary::channel_of(project, args::id(args, "channelId")?)?;
    let wanted = args::text(args, "preset", 4096)?.trim();
    let presets = call.host.channel_presets()?;
    let preset = presets
        .iter()
        .find(|preset| preset.path == wanted)
        .or_else(|| {
            presets
                .iter()
                .find(|preset| preset.name.eq_ignore_ascii_case(wanted))
        })
        .ok_or_else(|| {
            format!("there is no channel preset {wanted:?}; list_channel_presets lists them")
        })?;
    let mut draft = Draft::new("Load channel preset");
    draft.changes.push(format!(
        "load channel preset \"{}\" ({}) onto channel {}",
        preset.name, preset.instrument, channel.id.0
    ));
    let native = NativeEdit::ChannelPreset {
        channel: channel.id,
        path: preset.path.clone(),
        name: preset.name.clone(),
    };
    propose_native(call, snapshot, args, draft, Some(native))
}

fn pattern_and_channel<'a>(
    project: &'a Project,
    args: &Args,
) -> Result<(&'a windfall_project::Pattern, &'a windfall_project::Channel), String> {
    Ok((
        summary::pattern_of(project, args::id(args, "patternId")?)?,
        summary::channel_of(project, args::id(args, "channelId")?)?,
    ))
}

fn lane_notes(pattern: &windfall_project::Pattern, channel: ChannelId) -> &[Note] {
    pattern
        .lane(channel)
        .map(|lane| lane.notes.as_slice())
        .unwrap_or(&[])
}

/// A velocity from 0 to 1, or a MIDI velocity from 2 to 127.
fn velocity(value: &Value) -> Result<f32, String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("velocity {value} must be a number"))?;
    if (0.0..=1.0).contains(&number) {
        Ok(number as f32)
    } else if number.fract() == 0.0 && (2.0..=127.0).contains(&number) {
        Ok((number / 127.0) as f32)
    } else {
        Err(format!(
            "velocity {number} must be from 0 to 1, or a MIDI velocity from 2 to 127"
        ))
    }
}

/// Makes the pattern long enough for `end` ticks, in whole bars.
fn extend_pattern(draft: &mut Draft, pattern: &windfall_project::Pattern, grid: &Grid, end: u32) {
    let needed = end.div_ceil(TICKS_PER_STEP);
    if needed <= pattern.length_steps {
        return;
    }
    let bar = (grid.base().ticks_per_bar() / TICKS_PER_STEP).max(1);
    let steps = needed
        .div_ceil(bar)
        .saturating_mul(bar)
        .min(MAX_PATTERN_STEPS);
    draft.commands.push(Command::UpdatePattern {
        id: pattern.id,
        patch: PatternPatch {
            length_steps: Some(steps),
            ..Default::default()
        },
    });
    draft.changes.push(format!(
        "lengthen pattern {} from {} to {} steps",
        pattern.id.0, pattern.length_steps, steps
    ));
}

/// `create_pattern`.
pub fn create_pattern(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["name", "lengthSteps", "lengthBars", "color"]),
    )?;
    let project = snapshot.document.project();
    let name = args::optional_text(args, "name", 128)?.map(str::to_owned);
    let steps = match (
        args::optional_u32(args, "lengthSteps", 1..=MAX_PATTERN_STEPS)?,
        args::optional_u32(args, "lengthBars", 1..=256)?,
    ) {
        (Some(_), Some(_)) => return Err("give lengthSteps or lengthBars, not both".into()),
        (Some(steps), None) => Some(steps),
        (None, Some(bars)) => {
            let steps = bars * (project.settings.time_signature.ticks_per_bar() / TICKS_PER_STEP);
            if steps > MAX_PATTERN_STEPS {
                return Err(format!(
                    "{bars} bars are longer than the longest pattern, {MAX_PATTERN_STEPS} steps"
                ));
            }
            Some(steps)
        }
        (None, None) => None,
    };
    let color = args::optional_text(args, "color", 16)?
        .map(music::parse_color)
        .transpose()?;
    let mut ids = Ids::of(project);
    let mut draft = Draft::new("Create pattern");
    let id = ids.take();
    draft.expected.push(id);
    draft
        .commands
        .push(Command::AddPattern { name: name.clone() });
    if steps.is_some() || color.is_some() {
        draft.commands.push(Command::UpdatePattern {
            id: PatternId(id),
            patch: PatternPatch {
                name: None,
                color,
                length_steps: steps,
            },
        });
    }
    draft.changes.push(format!(
        "create pattern {id} {:?} of {} steps",
        name.as_deref().unwrap_or("Pattern N"),
        steps.unwrap_or(windfall_project::DEFAULT_PATTERN_STEPS)
    ));
    propose(call, snapshot, args, draft)
}

/// `add_notes`.
pub fn add_notes(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&[
            "patternId",
            "channelId",
            "notes",
            "replace",
            "extendPattern",
        ]),
    )?;
    let project = snapshot.document.project();
    let (pattern, channel) = pattern_and_channel(project, args)?;
    let grid = summary::pattern_grid(project, pattern);
    let replace = args::optional_text(args, "replace", 16)?.unwrap_or("none");
    if !["none", "lane", "overlap"].contains(&replace) {
        return Err("argument replace must be \"none\", \"lane\" or \"overlap\"".into());
    }
    let extend = args::optional_bool(args, "extendPattern")?.unwrap_or(true);
    let mut notes = Vec::new();
    for (index, value) in args::list(args, "notes", MAX_NOTES)?.iter().enumerate() {
        let note = args::item(value, "notes", index)?;
        args::known(note, &["pitch", "start", "length", "velocity", "pan"])
            .map_err(|error| format!("note {index}: {error}"))?;
        let at = |error: String| format!("note {index}: {error}");
        let key = music::pitch_value(args::value(note, "pitch").map_err(at)?).map_err(at)?;
        let start = grid
            .position_value(args::value(note, "start").map_err(at)?)
            .map_err(at)?;
        let length = match args::optional_value(note, "length") {
            Some(value) => grid.length_value(value).map_err(at)?,
            None => TICKS_PER_STEP,
        };
        let velocity = args::optional_value(note, "velocity")
            .map(velocity)
            .transpose()
            .map_err(at)?;
        let pan = args::optional_number(note, "pan").map_err(at)?;
        if pan.is_some_and(|pan| !(-1.0..=1.0).contains(&pan)) {
            return Err(at("pan must be from -1 (left) to 1 (right)".into()));
        }
        notes.push(NoteInit {
            start,
            length,
            key,
            velocity,
            pan: pan.map(|pan| pan as f32),
            expression: None,
        });
    }
    let existing = lane_notes(pattern, channel.id);
    let removed: Vec<NoteId> = match replace {
        "lane" => existing.iter().map(|note| note.id).collect(),
        "overlap" => {
            let first = notes.iter().map(|n| n.start).min().unwrap_or(0);
            let last = notes
                .iter()
                .map(|n| n.start.saturating_add(n.length))
                .max()
                .unwrap_or(0);
            existing
                .iter()
                .filter(|note| note.start >= first && note.start < last)
                .map(|note| note.id)
                .collect()
        }
        _ => Vec::new(),
    };
    let mut draft = Draft::new("Add notes");
    if !removed.is_empty() {
        draft.changes.push(format!(
            "remove {} notes of channel {} from pattern {}",
            removed.len(),
            channel.id.0,
            pattern.id.0
        ));
        draft.commands.push(Command::RemoveNotes {
            pattern: pattern.id,
            channel: channel.id,
            notes: removed,
        });
    }
    let mut ids = Ids::of(project);
    for note in &notes {
        draft.expected.push(ids.take());
        draft.changes.push(format!(
            "add {} at {} for {} ticks",
            music::pitch_name(note.key),
            grid.format_position(note.start),
            note.length
        ));
    }
    let end = notes
        .iter()
        .map(|note| note.start.saturating_add(note.length))
        .max()
        .unwrap_or(0);
    draft.commands.push(Command::AddNotes {
        pattern: pattern.id,
        channel: channel.id,
        notes,
    });
    if extend {
        extend_pattern(&mut draft, pattern, &grid, end);
    }
    propose(call, snapshot, args, draft)
}

/// The notes a removal or update picks: by id, or by range and pitch.
fn pick_notes<'a>(notes: &'a [Note], grid: &Grid, args: &Args) -> Result<Vec<&'a Note>, String> {
    if let Some(ids) = args::optional_list(args, "noteIds", MAX_NOTES)? {
        let mut picked = Vec::new();
        for value in ids {
            let id = value
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or_else(|| format!("noteIds must hold ids, not {value}"))?;
            picked.push(
                notes
                    .iter()
                    .find(|note| note.id.0 == id)
                    .ok_or_else(|| format!("there is no note {id} in this pattern and channel"))?,
            );
        }
        return Ok(picked);
    }
    let from = args::optional_value(args, "from")
        .map(|value| grid.position_value(value))
        .transpose()?;
    let to = args::optional_value(args, "to")
        .map(|value| grid.position_value(value))
        .transpose()?;
    let pitch = args::optional_value(args, "pitch")
        .map(music::pitch_value)
        .transpose()?;
    if from.is_none() && to.is_none() && pitch.is_none() {
        return Err("give noteIds, or from, to and pitch to pick the notes".into());
    }
    Ok(notes
        .iter()
        .filter(|note| from.is_none_or(|from| note.start >= from))
        .filter(|note| to.is_none_or(|to| note.start < to))
        .filter(|note| pitch.is_none_or(|pitch| note.key == pitch))
        .collect())
}

/// `remove_notes`.
pub fn remove_notes(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["patternId", "channelId", "noteIds", "from", "to", "pitch"]),
    )?;
    let project = snapshot.document.project();
    let (pattern, channel) = pattern_and_channel(project, args)?;
    let grid = summary::pattern_grid(project, pattern);
    let picked = pick_notes(lane_notes(pattern, channel.id), &grid, args)?;
    if picked.is_empty() {
        return Err("no notes match, so nothing would be removed".into());
    }
    let mut draft = Draft::new("Remove notes");
    draft.changes.push(format!(
        "remove {} notes of channel {} from pattern {}",
        picked.len(),
        channel.id.0,
        pattern.id.0
    ));
    draft.commands.push(Command::RemoveNotes {
        pattern: pattern.id,
        channel: channel.id,
        notes: picked.iter().map(|note| note.id).collect(),
    });
    propose(call, snapshot, args, draft)
}

/// `update_notes`.
pub fn update_notes(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["patternId", "channelId", "updates"]))?;
    let project = snapshot.document.project();
    let (pattern, channel) = pattern_and_channel(project, args)?;
    let grid = summary::pattern_grid(project, pattern);
    let notes = lane_notes(pattern, channel.id);
    let mut expected: Vec<Note> = Vec::new();
    let mut updates = Vec::new();
    let mut draft = Draft::new("Edit notes");
    for (index, value) in args::list(args, "updates", MAX_NOTES)?.iter().enumerate() {
        let update = args::item(value, "updates", index)?;
        let at = |error: String| format!("update {index}: {error}");
        args::known(
            update,
            &[
                "noteId",
                "pitch",
                "start",
                "length",
                "velocity",
                "pan",
                "transpose",
            ],
        )
        .map_err(at)?;
        let id = args::id(update, "noteId").map_err(at)?;
        let note = notes
            .iter()
            .find(|note| note.id.0 == id)
            .ok_or_else(|| at(format!("there is no note {id} in this pattern and channel")))?;
        if expected.iter().any(|known| known.id == note.id) {
            return Err(at(format!("note {id} is updated twice")));
        }
        let mut key = args::optional_value(update, "pitch")
            .map(music::pitch_value)
            .transpose()
            .map_err(at)?;
        if let Some(semitones) = args::optional_number(update, "transpose").map_err(at)? {
            if key.is_some() || semitones.fract() != 0.0 {
                return Err(at(
                    "transpose takes whole semitones and cannot go with pitch".into(),
                ));
            }
            let moved = i32::from(note.key) + semitones as i32;
            key = Some(
                u8::try_from(moved)
                    .ok()
                    .filter(|key| *key <= 127)
                    .ok_or_else(|| at(format!("transposing note {id} leaves the MIDI range")))?,
            );
        }
        let patch = NotePatch {
            start: args::optional_value(update, "start")
                .map(|value| grid.position_value(value))
                .transpose()
                .map_err(at)?,
            length: args::optional_value(update, "length")
                .map(|value| grid.length_value(value))
                .transpose()
                .map_err(at)?,
            key,
            velocity: args::optional_value(update, "velocity")
                .map(velocity)
                .transpose()
                .map_err(at)?,
            pan: args::optional_number(update, "pan")
                .map_err(at)?
                .map(|pan| pan.clamp(-1.0, 1.0) as f32),
            expression: None,
        };
        if patch == NotePatch::default() {
            return Err(at(
                "change at least one of pitch, transpose, start, length, velocity or pan".into(),
            ));
        }
        draft.changes.push(format!(
            "change note {id} ({} at {})",
            music::pitch_name(note.key),
            grid.format_position(note.start)
        ));
        expected.push(*note);
        updates.push(NoteUpdate { id: note.id, patch });
    }
    draft.commands.push(Command::UpdateCapturedNotes {
        pattern: pattern.id,
        channel: channel.id,
        expected,
        updates,
    });
    propose(call, snapshot, args, draft)
}

/// `set_steps`.
pub fn set_steps(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&[
            "patternId",
            "channelId",
            "steps",
            "firstStep",
            "pitch",
            "velocity",
            "accentVelocity",
        ]),
    )?;
    let project = snapshot.document.project();
    let (pattern, channel) = pattern_and_channel(project, args)?;
    let grid = summary::pattern_grid(project, pattern);
    let text = args::text(args, "steps", 4096)?;
    let first = args::optional_u32(args, "firstStep", 0..=MAX_PATTERN_STEPS - 1)?.unwrap_or(0);
    let key = args::optional_value(args, "pitch")
        .map(music::pitch_value)
        .transpose()?
        .unwrap_or(windfall_project::DEFAULT_KEY);
    let plain = args::optional_value(args, "velocity")
        .map(velocity)
        .transpose()?
        .unwrap_or(windfall_project::DEFAULT_VELOCITY);
    let accent = args::optional_value(args, "accentVelocity")
        .map(velocity)
        .transpose()?
        .unwrap_or(1.0);
    let notes = lane_notes(pattern, channel.id);
    let mut add = Vec::new();
    let mut remove = Vec::new();
    let mut step = first;
    for c in text.chars() {
        let level = match c {
            ' ' | '|' => continue,
            'x' | 'o' => Some(Some(plain)),
            'X' | 'O' | '!' => Some(Some(accent)),
            '.' | '-' => Some(None),
            '_' => None,
            other => {
                return Err(format!(
                    "steps may hold x (on), X (accent), . or - (off), _ (leave as it is), spaces and |, not {other:?}"
                ));
            }
        };
        if step >= MAX_PATTERN_STEPS {
            return Err(format!(
                "the steps run past step {MAX_PATTERN_STEPS}, the end of the longest pattern"
            ));
        }
        let start = step * TICKS_PER_STEP;
        let here: Vec<&Note> = notes.iter().filter(|note| note.start == start).collect();
        match level {
            Some(Some(velocity)) if here.is_empty() => add.push(NoteInit {
                start,
                length: TICKS_PER_STEP,
                key,
                velocity: Some(velocity),
                pan: None,
                expression: None,
            }),
            Some(None) => remove.extend(here.iter().map(|note| note.id)),
            _ => {}
        }
        step += 1;
    }
    let mut draft = Draft::new("Set steps");
    if !remove.is_empty() {
        draft
            .changes
            .push(format!("turn off {} steps", remove.len()));
        draft.commands.push(Command::RemoveNotes {
            pattern: pattern.id,
            channel: channel.id,
            notes: remove,
        });
    }
    if !add.is_empty() {
        let mut ids = Ids::of(project);
        for _ in &add {
            draft.expected.push(ids.take());
        }
        draft.changes.push(format!(
            "turn on {} steps with {}",
            add.len(),
            music::pitch_name(key)
        ));
        draft.commands.push(Command::AddNotes {
            pattern: pattern.id,
            channel: channel.id,
            notes: add,
        });
    }
    extend_pattern(&mut draft, pattern, &grid, step * TICKS_PER_STEP);
    if draft.commands.is_empty() {
        return Err("those steps are already set, so nothing would change".into());
    }
    propose(call, snapshot, args, draft)
}

/// A built-in kind, by its command name ("subtractiveSynth") or the name
/// shown in Windfall.
fn parse_kind<T>(
    text: &str,
    all: &[T],
    name: impl Fn(&T) -> &'static str,
    what: &str,
) -> Result<T, String>
where
    T: Copy + serde::de::DeserializeOwned,
{
    if let Ok(kind) = serde_json::from_value::<T>(json!(text)) {
        return Ok(kind);
    }
    all.iter()
        .copied()
        .find(|kind| name(kind).eq_ignore_ascii_case(text.trim()))
        .ok_or_else(|| format!("{text:?} is not a {what}; list_instruments_and_effects lists them"))
}

/// `add_channel`.
pub fn add_channel(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["instrument", "name", "mixerTrackId", "soundFont"]),
    )?;
    let project = snapshot.document.project();
    let soundfont = match args::optional_value(args, "soundFont") {
        None => None,
        Some(Value::Object(choice)) => {
            args::known(choice, &["path", "bank", "program"])?;
            Some(soundfont_choice(call, choice)?)
        }
        Some(_) => {
            return Err("argument soundFont must be {\"path\", \"bank\", \"program\"}".into());
        }
    };
    let instrument = match (args::optional_text(args, "instrument", 64)?, &soundfont) {
        (Some(text), _) => parse_kind(
            text,
            &InstrumentKind::ALL,
            |kind| kind.name(),
            "built-in instrument",
        )?,
        (None, Some(_)) => InstrumentKind::SoundFont,
        (None, None) => return Err("argument instrument is required".into()),
    };
    if soundfont.is_some() && instrument != InstrumentKind::SoundFont {
        return Err(
            "soundFont is for a SoundFont channel: leave instrument out or make it \"soundFont\""
                .into(),
        );
    }
    let name = args::optional_text(args, "name", 128)?.map(str::to_owned);
    let mixer_track = args::optional_id(args, "mixerTrackId")?
        .map(|id| summary::track_of(project, id).map(|track| track.id))
        .transpose()?;
    let mut ids = Ids::of(project);
    let mut draft = Draft::new("Add channel");
    let id = ids.take();
    draft.expected.push(id);
    draft.changes.push(format!(
        "add channel {id} playing {} into {}",
        instrument.name(),
        mixer_track.map_or("a new mixer track".to_owned(), |track| format!(
            "mixer track {}",
            track.0
        ))
    ));
    draft.commands.push(Command::AddChannel {
        name,
        sample: None,
        instrument: Some(instrument),
        index: None,
        mixer_track,
    });
    let native = soundfont.map(|choice| {
        draft.changes.push(format!(
            "load SoundFont preset \"{}\" (bank {}, program {}) from {} into channel {id}",
            choice.preset.name, choice.preset.bank, choice.preset.program, choice.path
        ));
        NativeEdit::SoundFont {
            channel: ChannelId(id),
            path: choice.path,
            bank: choice.preset.bank,
            program: choice.preset.program,
            preset: choice.preset.name,
            replace_instrument: false,
        }
    });
    let silent = native
        .is_none()
        .then(|| summary::source_needed(instrument))
        .flatten();
    let mut result = propose_native(call, snapshot, args, draft, native)?;
    if let Some(needed) = silent {
        result["warning"] = json!(format!(
            "channel {id} will be silent until it has a source: it needs {needed}"
        ));
    }
    Ok(result)
}

/// `add_effect`.
pub fn add_effect(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["trackId", "kind", "index"]))?;
    let project = snapshot.document.project();
    let track = summary::track_of(project, args::id(args, "trackId")?)?;
    let kind = parse_kind(
        args::text(args, "kind", 64)?,
        &EffectKind::ALL,
        |kind| kind.name(),
        "built-in effect",
    )?;
    if track.effects.len() >= windfall_project::MAX_EFFECT_SLOTS {
        return Err(format!(
            "mixer track {} already holds {} effects, the most a track holds",
            track.id.0,
            windfall_project::MAX_EFFECT_SLOTS
        ));
    }
    let index = args::optional_u32(args, "index", 0..=windfall_project::MAX_EFFECT_SLOTS as u32)?;
    let mut draft = Draft::new("Add effect");
    let id = Ids::of(project).take();
    draft.expected.push(id);
    draft.changes.push(format!(
        "add {} as effect {id} on mixer track {}",
        kind.name(),
        track.id.0
    ));
    draft.commands.push(Command::AddEffect {
        track: track.id,
        kind,
        index,
    });
    propose(call, snapshot, args, draft)
}

fn effect_on(project: &Project, args: &Args) -> Result<(TrackId, EffectId, EffectKind), String> {
    let track = summary::track_of(project, args::id(args, "trackId")?)?;
    let id = args::id(args, "effectId")?;
    let effect = track
        .effect(EffectId(id))
        .ok_or_else(|| format!("mixer track {} has no effect {id}", track.id.0))?;
    Ok((track.id, effect.id, effect.params.kind()))
}

/// `remove_effect`.
pub fn remove_effect(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["trackId", "effectId"]))?;
    let (track, effect, kind) = effect_on(snapshot.document.project(), args)?;
    let mut draft = Draft::new("Remove effect");
    draft.changes.push(format!(
        "remove {} (effect {}) from mixer track {}",
        kind.name(),
        effect.0,
        track.0
    ));
    draft.commands.push(Command::RemoveEffect { track, effect });
    propose(call, snapshot, args, draft)
}

/// `move_effect`.
pub fn move_effect(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["trackId", "effectId", "index", "toTrackId"]),
    )?;
    let project = snapshot.document.project();
    let (track, effect, kind) = effect_on(project, args)?;
    let to_track = args::optional_id(args, "toTrackId")?
        .map(|id| summary::track_of(project, id).map(|track| track.id))
        .transpose()?;
    let index = args::optional_u32(args, "index", 0..=windfall_project::MAX_EFFECT_SLOTS as u32)?
        .ok_or("argument index is required")?;
    let mut draft = Draft::new("Move effect");
    draft.changes.push(format!(
        "move {} (effect {}) to slot {index} of mixer track {}",
        kind.name(),
        effect.0,
        to_track.unwrap_or(track).0
    ));
    draft.commands.push(Command::MoveEffect {
        track,
        effect,
        to_track,
        index,
    });
    propose(call, snapshot, args, draft)
}

/// A gain: a linear factor from 0 to 2, or text in decibels ("-6 dB").
fn gain(value: &Value) -> Result<f32, String> {
    let linear = match value {
        Value::String(text) => {
            let number = text
                .trim()
                .strip_suffix("dB")
                .or_else(|| text.trim().strip_suffix("db"))
                .map(str::trim)
                .and_then(|db| db.parse::<f64>().ok())
                .ok_or_else(|| format!("{text:?} is not a level; write a factor such as 0.5 or decibels such as \"-6 dB\""))?;
            10f64.powf(number / 20.0)
        }
        other => other.as_f64().ok_or_else(|| {
            format!("level {other} must be a number or decibels such as \"-6 dB\"")
        })?,
    };
    if !linear.is_finite() || !(0.0..=f64::from(MAX_GAIN)).contains(&linear) {
        return Err(format!(
            "level {linear:.3} is outside 0 to {MAX_GAIN} (silence to +6 dB)"
        ));
    }
    Ok(linear as f32)
}

fn pan(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .filter(|pan| (-1.0..=1.0).contains(pan))
        .map(|pan| pan as f32)
        .ok_or_else(|| format!("pan {value} must be from -1 (left) to 1 (right)"))
}

fn flag(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(n) if n.as_f64() == Some(0.0) => Ok(false),
        Value::Number(n) if n.as_f64() == Some(1.0) => Ok(true),
        other => Err(format!("{other} must be true or false")),
    }
}

/// Finds a setting by index, id or shown name.
pub fn find_param(
    descriptors: &'static [ParamInfo],
    wanted: &Value,
) -> Result<(usize, &'static ParamInfo), String> {
    if let Some(index) = wanted.as_u64() {
        return descriptors
            .get(index as usize)
            .map(|info| (index as usize, info))
            .ok_or_else(|| {
                format!(
                    "there is no setting {index}; there are {}",
                    descriptors.len()
                )
            });
    }
    let text = wanted
        .as_str()
        .ok_or_else(|| format!("parameter {wanted} must be a name, an id or an index"))?
        .trim();
    if let Some(found) = descriptors.iter().position(|info| info.id == text) {
        return Ok((found, &descriptors[found]));
    }
    let named: Vec<usize> = descriptors
        .iter()
        .enumerate()
        .filter(|(_, info)| info.name.eq_ignore_ascii_case(text))
        .map(|(index, _)| index)
        .collect();
    match named.as_slice() {
        [index] => Ok((*index, &descriptors[*index])),
        [] => {
            let lower = text.to_lowercase();
            let similar: Vec<&str> = descriptors
                .iter()
                .filter(|info| {
                    info.name.to_lowercase().contains(&lower)
                        || info.id.to_lowercase().contains(&lower)
                })
                .take(8)
                .map(|info| info.id)
                .collect();
            Err(format!(
                "there is no setting {text:?}; similar ids: {similar:?}. get_parameters lists them all"
            ))
        }
        several => Err(format!(
            "{text:?} names {} settings; use one of the ids {:?}",
            several.len(),
            several
                .iter()
                .map(|index| descriptors[*index].id)
                .collect::<Vec<_>>()
        )),
    }
}

/// A value in a setting's own terms, checked against its range: a number,
/// a toggle as true or false, a choice by its label or index.
pub fn param_value(info: &ParamInfo, value: &Value) -> Result<f32, String> {
    match info.kind {
        ParamKind::Toggle => flag(value)
            .map(|on| if on { 1.0 } else { 0.0 })
            .map_err(|_| format!("{} is a toggle: give true or false", info.name)),
        ParamKind::Choice => {
            if let Some(text) = value.as_str() {
                return info
                    .choices
                    .iter()
                    .position(|choice| {
                        choice.label.eq_ignore_ascii_case(text.trim())
                            || choice.value.eq_ignore_ascii_case(text.trim())
                    })
                    .map(|index| index as f32)
                    .ok_or_else(|| {
                        format!(
                            "{} has no choice {text:?}; its choices are {:?}",
                            info.name,
                            info.choices.iter().map(|c| c.label).collect::<Vec<_>>()
                        )
                    });
            }
            value
                .as_u64()
                .filter(|index| (*index as usize) < info.choices.len())
                .map(|index| index as f32)
                .ok_or_else(|| {
                    format!(
                        "{} takes a choice label or an index from 0 to {}",
                        info.name,
                        info.choices.len().saturating_sub(1)
                    )
                })
        }
        ParamKind::Integer | ParamKind::Float => {
            let number = value
                .as_f64()
                .ok_or_else(|| format!("{} takes a number, not {value}", info.name))?;
            let tolerance = f64::from((info.max - info.min).abs()) * 1e-6;
            if !(f64::from(info.min) - tolerance..=f64::from(info.max) + tolerance)
                .contains(&number)
            {
                return Err(format!(
                    "{} takes {} to {} ({}); {number} is outside",
                    info.name,
                    info.min,
                    info.max,
                    kind_name(&info.unit)
                ));
            }
            if info.kind == ParamKind::Integer && number.fract() != 0.0 {
                return Err(format!("{} takes whole numbers", info.name));
            }
            Ok((number as f32).clamp(info.min, info.max))
        }
    }
}

/// `set_parameter`: one setting, or several of one target.
pub fn set_parameter(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&[
            "channelId",
            "trackId",
            "effectId",
            "parameter",
            "value",
            "parameters",
        ]),
    )?;
    let project = snapshot.document.project();
    let mut pairs: Vec<(&Value, &Value)> = Vec::new();
    match (
        args::optional_value(args, "parameter"),
        args::optional_list(args, "parameters", 64)?,
    ) {
        (Some(parameter), None) => pairs.push((parameter, args::value(args, "value")?)),
        (None, Some(list)) => {
            for (index, item) in list.iter().enumerate() {
                let item = args::item(item, "parameters", index)?;
                args::known(item, &["parameter", "value"])
                    .map_err(|e| format!("parameters {index}: {e}"))?;
                pairs.push((
                    args::value(item, "parameter")
                        .map_err(|e| format!("parameters {index}: {e}"))?,
                    args::value(item, "value").map_err(|e| format!("parameters {index}: {e}"))?,
                ));
            }
        }
        _ => return Err("give parameter and value, or a list of parameters".into()),
    }
    let channel = args::optional_id(args, "channelId")?;
    let track = args::optional_id(args, "trackId")?;
    let effect = args::optional_id(args, "effectId")?;
    let mut draft = Draft::new("Change settings");
    let mut channel_patch = ChannelPatch::default();
    let mut track_patch = MixerTrackPatch::default();
    let mut slot_patch = EffectSlotPatch::default();
    let mut target = None;
    for (parameter, value) in pairs {
        let special = parameter.as_str().map(str::to_ascii_lowercase);
        match (special.as_deref(), channel, track, effect) {
            (Some("volume"), Some(id), None, None) => {
                summary::channel_of(project, id)?;
                channel_patch.volume = Some(gain(value)?);
                draft
                    .changes
                    .push(format!("set channel {id} volume to {value}"));
            }
            (Some("pan"), Some(id), None, None) => {
                summary::channel_of(project, id)?;
                channel_patch.pan = Some(pan(value)?);
                draft
                    .changes
                    .push(format!("set channel {id} pan to {value}"));
            }
            (Some("muted" | "mute"), Some(id), None, None) => {
                summary::channel_of(project, id)?;
                channel_patch.muted = Some(flag(value)?);
                draft
                    .changes
                    .push(format!("set channel {id} muted to {value}"));
            }
            (Some("volume"), None, Some(id), None) => {
                summary::track_of(project, id)?;
                track_patch.volume = Some(gain(value)?);
                draft
                    .changes
                    .push(format!("set mixer track {id} volume to {value}"));
            }
            (Some("pan"), None, Some(id), None) => {
                summary::track_of(project, id)?;
                track_patch.pan = Some(pan(value)?);
                draft
                    .changes
                    .push(format!("set mixer track {id} pan to {value}"));
            }
            (Some("muted" | "mute"), None, Some(id), None) => {
                summary::track_of(project, id)?;
                track_patch.muted = Some(flag(value)?);
                draft
                    .changes
                    .push(format!("set mixer track {id} muted to {value}"));
            }
            (Some("mix"), None, Some(_), Some(_)) => {
                let (_, effect, _) = effect_on(project, args)?;
                let mix = value
                    .as_f64()
                    .filter(|mix| (0.0..=1.0).contains(mix))
                    .ok_or_else(|| {
                        format!("mix {value} must be from 0 (dry) to 1 (effect only)")
                    })?;
                slot_patch.mix = Some(mix as f32);
                draft
                    .changes
                    .push(format!("set effect {} mix to {mix}", effect.0));
            }
            (Some("enabled"), None, Some(_), Some(_)) => {
                let (_, effect, _) = effect_on(project, args)?;
                slot_patch.enabled = Some(flag(value)?);
                draft
                    .changes
                    .push(format!("set effect {} enabled to {value}", effect.0));
            }
            _ => {
                if target.is_none() {
                    target = Some(ParamTarget::find(project, args)?);
                }
                let found = target.as_ref().expect("the target was found above");
                let (index, info) = find_param(found.descriptors(), parameter)?;
                let number = param_value(info, value)?;
                let old = found.value(project, index);
                draft.changes.push(format!(
                    "set {} from {} to {number}",
                    info.name,
                    old.map_or("?".to_owned(), |old| old.to_string())
                ));
                let param = index as u32;
                draft.commands.push(match found {
                    ParamTarget::Instrument { channel, .. } => Command::SetInstrumentParam {
                        channel: *channel,
                        param,
                        value: number,
                    },
                    ParamTarget::Effect { track, effect, .. } => Command::SetEffectParam {
                        track: *track,
                        effect: *effect,
                        param,
                        value: number,
                    },
                    ParamTarget::Track { track } => Command::SetTrackParam {
                        id: *track,
                        param,
                        value: number,
                    },
                });
            }
        }
    }
    if channel_patch != ChannelPatch::default()
        && let Some(id) = channel
    {
        draft.commands.push(Command::UpdateChannel {
            id: ChannelId(id),
            patch: channel_patch,
        });
    }
    if track_patch != MixerTrackPatch::default()
        && let Some(id) = track
    {
        draft.commands.push(Command::UpdateMixerTrack {
            id: TrackId(id),
            patch: track_patch,
        });
    }
    if slot_patch != EffectSlotPatch::default() {
        let (track, effect, _) = effect_on(project, args)?;
        draft.commands.push(Command::UpdateEffect {
            track,
            effect,
            patch: slot_patch,
        });
    }
    propose(call, snapshot, args, draft)
}

/// `route_channel`.
pub fn route_channel(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["channelId", "trackId"]))?;
    let project = snapshot.document.project();
    let channel = summary::channel_of(project, args::id(args, "channelId")?)?;
    let track = summary::track_of(project, args::id(args, "trackId")?)?;
    let mut draft = Draft::new("Route channel");
    draft.changes.push(format!(
        "route channel {} from mixer track {} to mixer track {}",
        channel.id.0, channel.mixer_track.0, track.id.0
    ));
    draft.commands.push(Command::UpdateChannel {
        id: channel.id,
        patch: ChannelPatch {
            mixer_track: Some(track.id),
            ..Default::default()
        },
    });
    propose(call, snapshot, args, draft)
}

/// `route_track`.
pub fn route_track(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["trackId", "outputId"]))?;
    let project = snapshot.document.project();
    let track = summary::track_of(project, args::id(args, "trackId")?)?;
    let output = summary::track_of(project, args::id(args, "outputId")?)?;
    if track.id == TrackId::MASTER {
        return Err("the master track has no output to route".into());
    }
    let mut draft = Draft::new("Route mixer track");
    draft.changes.push(format!(
        "route mixer track {} to mixer track {}",
        track.id.0, output.id.0
    ));
    draft.commands.push(Command::SetTrackOutput {
        id: track.id,
        output: Some(output.id),
    });
    propose(call, snapshot, args, draft)
}

/// `set_send`.
pub fn set_send(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&["fromTrackId", "toTrackId", "level", "remove"]),
    )?;
    let project = snapshot.document.project();
    let from = summary::track_of(project, args::id(args, "fromTrackId")?)?;
    let to = summary::track_of(project, args::id(args, "toTrackId")?)?;
    let remove = args::optional_bool(args, "remove")?.unwrap_or(false);
    let level = match (remove, args::optional_value(args, "level")) {
        (true, None) => None,
        (false, Some(level)) => Some(gain(level)?),
        (true, Some(_)) => return Err("give level or remove, not both".into()),
        (false, None) => return Err("give the send's level, or remove: true".into()),
    };
    if remove && !from.sends.iter().any(|send| send.target == to.id) {
        return Err(format!(
            "mixer track {} has no send to mixer track {}",
            from.id.0, to.id.0
        ));
    }
    let mut draft = Draft::new("Set send");
    draft.changes.push(match level {
        Some(level) => format!(
            "send mixer track {} to mixer track {} at {level:.3}",
            from.id.0, to.id.0
        ),
        None => format!(
            "remove the send from mixer track {} to mixer track {}",
            from.id.0, to.id.0
        ),
    });
    draft.commands.push(Command::SetSend {
        from: from.id,
        to: to.id,
        gain: level,
    });
    propose(call, snapshot, args, draft)
}

fn playlist_track(project: &Project, id: u32) -> Result<PlaylistTrackId, String> {
    project
        .playlist
        .tracks
        .iter()
        .find(|track| track.id.0 == id)
        .map(|track| track.id)
        .ok_or_else(|| format!("there is no playlist track {id}"))
}

/// `place_clips`.
pub fn place_clips(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["clips", "newTrackName"]))?;
    let project = snapshot.document.project();
    let grid = summary::song_grid(project);
    let mut ids = Ids::of(project);
    let mut draft = Draft::new("Place clips");
    let items = args::list(args, "clips", MAX_CLIPS)?;
    let new_track = if items
        .iter()
        .any(|item| item.get("trackId").is_none_or(Value::is_null))
    {
        let id = ids.take();
        draft.expected.push(id);
        let name = args::optional_text(args, "newTrackName", 128)?.map(str::to_owned);
        draft.changes.push(format!("add playlist track {id}"));
        draft
            .commands
            .push(Command::AddPlaylistTrack { name, index: None });
        Some(PlaylistTrackId(id))
    } else {
        None
    };
    let mut clips = Vec::new();
    for (index, value) in items.iter().enumerate() {
        let item = args::item(value, "clips", index)?;
        let at = |error: String| format!("clip {index}: {error}");
        args::known(
            item,
            &[
                "patternId",
                "automationId",
                "trackId",
                "start",
                "length",
                "offset",
                "muted",
            ],
        )
        .map_err(at)?;
        let content = match (
            args::optional_id(item, "patternId").map_err(at)?,
            args::optional_id(item, "automationId").map_err(at)?,
        ) {
            (Some(pattern), None) => ClipContent::Pattern {
                pattern: summary::pattern_of(project, pattern).map_err(at)?.id,
            },
            (None, Some(automation)) => ClipContent::Automation {
                automation: project
                    .automation(AutomationId(automation))
                    .ok_or_else(|| at(format!("there is no automation {automation}")))?
                    .id,
            },
            _ => return Err(at("give patternId or automationId".into())),
        };
        let track = match args::optional_id(item, "trackId").map_err(at)? {
            Some(id) => playlist_track(project, id).map_err(at)?,
            None => new_track.expect("a new track is made for clips without one"),
        };
        let start = grid
            .position_value(args::value(item, "start").map_err(at)?)
            .map_err(at)?;
        let length = args::optional_value(item, "length")
            .map(|value| grid.length_value(value))
            .transpose()
            .map_err(at)?;
        let offset = args::optional_value(item, "offset")
            .map(|value| grid.length_value(value))
            .transpose()
            .map_err(at)?;
        draft.expected.push(ids.take());
        draft.changes.push(format!(
            "place a clip on playlist track {} at {}",
            track.0,
            grid.format_position(start)
        ));
        clips.push(ClipInit {
            track,
            start,
            length,
            offset,
            muted: args::optional_bool(item, "muted").map_err(at)?,
            content,
        });
    }
    draft.commands.push(Command::AddClips { clips });
    propose(call, snapshot, args, draft)
}

/// `move_clips`.
pub fn move_clips(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["clips"]))?;
    let project = snapshot.document.project();
    let grid = summary::song_grid(project);
    let mut updates = Vec::new();
    let mut draft = Draft::new("Move clips");
    for (index, value) in args::list(args, "clips", MAX_CLIPS)?.iter().enumerate() {
        let item = args::item(value, "clips", index)?;
        let at = |error: String| format!("clip {index}: {error}");
        args::known(
            item,
            &["clipId", "trackId", "start", "length", "offset", "muted"],
        )
        .map_err(at)?;
        let id = args::id(item, "clipId").map_err(at)?;
        let clip = project
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id.0 == id)
            .ok_or_else(|| at(format!("there is no clip {id}; list_clips lists them")))?;
        let patch = ClipPatch {
            track: args::optional_id(item, "trackId")
                .map_err(at)?
                .map(|id| playlist_track(project, id))
                .transpose()
                .map_err(at)?,
            start: args::optional_value(item, "start")
                .map(|value| grid.position_value(value))
                .transpose()
                .map_err(at)?,
            length: args::optional_value(item, "length")
                .map(|value| grid.length_value(value))
                .transpose()
                .map_err(at)?,
            offset: args::optional_value(item, "offset")
                .map(|value| grid.length_value(value))
                .transpose()
                .map_err(at)?,
            muted: args::optional_bool(item, "muted").map_err(at)?,
        };
        if patch == ClipPatch::default() {
            return Err(at(
                "change at least one of trackId, start, length, offset or muted".into(),
            ));
        }
        draft.changes.push(format!(
            "change clip {id} at {}",
            grid.format_position(clip.start)
        ));
        updates.push(ClipUpdate {
            id: ClipId(id),
            patch,
        });
    }
    draft.commands.push(Command::UpdateClips { updates });
    propose(call, snapshot, args, draft)
}

/// `remove_clips`.
pub fn remove_clips(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["clipIds"]))?;
    let project = snapshot.document.project();
    let ids = args::ids(args, "clipIds", MAX_CLIPS)?;
    for id in &ids {
        if !project.playlist.clips.iter().any(|clip| clip.id.0 == *id) {
            return Err(format!("there is no clip {id}; list_clips lists them"));
        }
    }
    let mut draft = Draft::new("Remove clips");
    draft.changes.push(format!("remove clips {ids:?}"));
    draft.commands.push(Command::RemoveClips {
        clips: ids.into_iter().map(ClipId).collect(),
    });
    propose(call, snapshot, args, draft)
}

/// `set_tempo_meter`.
pub fn set_tempo_meter(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["tempoBpm", "meter"]))?;
    let project = snapshot.document.project();
    let tempo = args::optional_number(args, "tempoBpm")?;
    if tempo.is_some_and(|tempo| !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&tempo)) {
        return Err(format!(
            "tempoBpm must be from {MIN_TEMPO_BPM} to {MAX_TEMPO_BPM}"
        ));
    }
    let meter = args::optional_text(args, "meter", 8)?
        .map(music::parse_meter)
        .transpose()?;
    if tempo.is_none() && meter.is_none() {
        return Err("give tempoBpm, meter or both".into());
    }
    let mut draft = Draft::new("Set tempo and meter");
    if let Some(tempo) = tempo {
        draft.changes.push(format!(
            "set the tempo from {} to {tempo} bpm",
            project.settings.tempo_bpm
        ));
    }
    if let Some(meter) = meter {
        draft.changes.push(format!(
            "set the meter from {} to {}",
            music::format_meter(project.settings.time_signature),
            music::format_meter(meter)
        ));
    }
    draft.commands.push(Command::UpdateSettings {
        patch: SettingsPatch {
            tempo_bpm: tempo,
            time_signature: meter,
            ..Default::default()
        },
    });
    propose(call, snapshot, args, draft)
}

/// `rename_and_color`.
pub fn rename_and_color(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
) -> Result<Value, String> {
    args::known(args, &allowed(&["target", "id", "name", "color"]))?;
    let project = snapshot.document.project();
    let target = args::text(args, "target", 32)?;
    let id = args::id(args, "id")?;
    let name = args::optional_text(args, "name", 128)?
        .map(str::trim)
        .map(|name| {
            if name.is_empty() {
                Err("a name cannot be empty")
            } else {
                Ok(name.to_owned())
            }
        })
        .transpose()?;
    let color = args::optional_text(args, "color", 16)?
        .map(music::parse_color)
        .transpose()?;
    if name.is_none() && color.is_none() {
        return Err("give a name, a color or both".into());
    }
    let mut draft = Draft::new("Rename");
    draft
        .changes
        .push(format!("rename or recolour {target} {id}"));
    draft.commands.push(match target {
        "channel" => Command::UpdateChannel {
            id: summary::channel_of(project, id)?.id,
            patch: ChannelPatch {
                name,
                color,
                ..Default::default()
            },
        },
        "pattern" => Command::UpdatePattern {
            id: summary::pattern_of(project, id)?.id,
            patch: PatternPatch {
                name,
                color,
                length_steps: None,
            },
        },
        "mixerTrack" => Command::UpdateMixerTrack {
            id: summary::track_of(project, id)?.id,
            patch: MixerTrackPatch {
                name,
                color,
                ..Default::default()
            },
        },
        "playlistTrack" => Command::UpdatePlaylistTrack {
            id: playlist_track(project, id)?,
            patch: PlaylistTrackPatch {
                name,
                color,
                ..Default::default()
            },
        },
        _ => {
            return Err("argument target must be \"channel\", \"pattern\", \"mixerTrack\" or \"playlistTrack\"".into());
        }
    });
    propose(call, snapshot, args, draft)
}

/// The automation target named by `target`, and the setting it moves
/// when it is one.
fn automation_target(
    project: &Project,
    target: &Args,
) -> Result<(AutomationTarget, Option<&'static ParamInfo>), String> {
    args::known(
        target,
        &["channelId", "trackId", "effectId", "sendTo", "parameter"],
    )?;
    let parameter = args::value(target, "parameter")?;
    let name = parameter.as_str().map(str::to_ascii_lowercase);
    let channel = args::optional_id(target, "channelId")?;
    let track = args::optional_id(target, "trackId")?;
    let effect = args::optional_id(target, "effectId")?;
    let send = args::optional_id(target, "sendTo")?;
    let found = match (name.as_deref(), channel, track, effect, send) {
        (Some("tempo"), None, None, None, None) => AutomationTarget::Tempo,
        (Some("volume"), Some(id), None, None, None) => AutomationTarget::ChannelVolume {
            channel: summary::channel_of(project, id)?.id,
        },
        (Some("pan"), Some(id), None, None, None) => AutomationTarget::ChannelPan {
            channel: summary::channel_of(project, id)?.id,
        },
        (Some("volume"), None, Some(id), None, None) => AutomationTarget::TrackVolume {
            track: summary::track_of(project, id)?.id,
        },
        (Some("pan"), None, Some(id), None, None) => AutomationTarget::TrackPan {
            track: summary::track_of(project, id)?.id,
        },
        (Some("level"), None, Some(from), None, Some(to)) => AutomationTarget::SendGain {
            track: summary::track_of(project, from)?.id,
            target: summary::track_of(project, to)?.id,
        },
        (Some("mix"), None, Some(_), Some(_), None) => {
            let (track, effect, _) = effect_on(project, target)?;
            AutomationTarget::EffectMix { track, effect }
        }
        (_, _, _, _, None) => {
            let found = ParamTarget::find(project, target)?;
            let (index, info) = find_param(found.descriptors(), parameter)?;
            let param = index as u32;
            let target = match found {
                ParamTarget::Instrument { channel, .. } => {
                    AutomationTarget::InstrumentParam { channel, param }
                }
                ParamTarget::Effect { track, effect, .. } => AutomationTarget::EffectParam {
                    track,
                    effect,
                    param,
                },
                ParamTarget::Track { track } => AutomationTarget::TrackParam { track, param },
            };
            return Ok((target, Some(info)));
        }
        _ => return Err("sendTo goes with trackId and parameter \"level\"".into()),
    };
    Ok((found, None))
}

/// `write_automation`.
pub fn write_automation(
    call: &Call<'_>,
    snapshot: &Snapshot,
    args: &Args,
) -> Result<Value, String> {
    args::known(
        args,
        &allowed(&[
            "target",
            "points",
            "automationId",
            "name",
            "playlistTrackId",
            "placeClip",
        ]),
    )?;
    let project = snapshot.document.project();
    let grid = summary::song_grid(project);
    let target_args = args::value(args, "target")?
        .as_object()
        .ok_or("argument target must be an object")?;
    let (target, info) = automation_target(project, target_args)?;
    let range = project
        .automation_range(&target)
        .ok_or("the project has no such automation target")?;
    let existing = args::optional_id(args, "automationId")?
        .map(|id| {
            project
                .automation(AutomationId(id))
                .ok_or_else(|| format!("there is no automation {id}"))
        })
        .transpose()?;
    if let Some(existing) = existing
        && existing.target != target
    {
        return Err(format!(
            "automation {} moves another target; leave automationId out to make a new one",
            existing.id.0
        ));
    }
    let mut points = Vec::new();
    for (index, value) in args::list(args, "points", MAX_POINTS)?.iter().enumerate() {
        let point = args::item(value, "points", index)?;
        let at = |error: String| format!("point {index}: {error}");
        args::known(point, &["at", "value", "curve", "hold"]).map_err(at)?;
        let tick = grid
            .position_value(args::value(point, "at").map_err(at)?)
            .map_err(at)?;
        let raw = args::value(point, "value").map_err(at)?;
        let value = match (info, &target) {
            (Some(info), _) => param_value(info, raw).map_err(at)?,
            (
                None,
                AutomationTarget::ChannelVolume { .. }
                | AutomationTarget::TrackVolume { .. }
                | AutomationTarget::SendGain { .. },
            ) => gain(raw).map_err(at)?,
            (None, AutomationTarget::ChannelPan { .. } | AutomationTarget::TrackPan { .. }) => {
                pan(raw).map_err(at)?
            }
            (None, _) => {
                let number = raw
                    .as_f64()
                    .ok_or_else(|| at(format!("value {raw} must be a number")))?;
                if !(f64::from(range.min)..=f64::from(range.max)).contains(&number) {
                    return Err(at(format!(
                        "value {number} is outside {} to {}",
                        range.min, range.max
                    )));
                }
                number as f32
            }
        };
        let curve = args::optional_number(point, "curve")
            .map_err(at)?
            .unwrap_or(0.0);
        if !(-1.0..=1.0).contains(&curve) {
            return Err(at("curve must be from -1 to 1".into()));
        }
        points.push((
            tick,
            AutomationPoint {
                tick,
                value: range.normalized(value),
                curve: curve as f32,
                hold: args::optional_bool(point, "hold")
                    .map_err(at)?
                    .unwrap_or(false),
            },
        ));
    }
    points.sort_by_key(|(tick, _)| *tick);
    let first = points[0].0;
    let last = points[points.len() - 1].0;
    let mut draft = Draft::new("Write automation");
    let mut ids = Ids::of(project);
    match existing {
        Some(existing) => {
            // Song positions become curve ticks through the automation's
            // first clip, or are curve ticks when it has none.
            let shift = project
                .playlist
                .clips
                .iter()
                .filter(|clip| clip.content.automation() == Some(existing.id))
                .min_by_key(|clip| clip.start)
                .map_or(0i64, |clip| i64::from(clip.offset) - i64::from(clip.start));
            let curve: Vec<AutomationPoint> = points
                .iter()
                .map(|(tick, point)| {
                    let tick =
                        (i64::from(*tick) + shift).clamp(0, i64::from(MAX_SONG_TICKS)) as u32;
                    AutomationPoint { tick, ..*point }
                })
                .collect();
            draft.changes.push(format!(
                "replace the {} points of automation {} with {}",
                existing.points.len(),
                existing.id.0,
                curve.len()
            ));
            draft.commands.push(Command::SetAutomationPoints {
                id: existing.id,
                points: curve,
            });
        }
        None => {
            let id = ids.take();
            draft.expected.push(id);
            let curve: Vec<AutomationPoint> = points
                .iter()
                .map(|(tick, point)| AutomationPoint {
                    tick: tick - first,
                    ..*point
                })
                .collect();
            let name = args::optional_text(args, "name", 128)?.map(str::to_owned);
            draft.changes.push(format!(
                "add automation {id} with {} points from {} to {}",
                curve.len(),
                grid.format_position(first),
                grid.format_position(last)
            ));
            draft.commands.push(Command::AddAutomation {
                name,
                target,
                points: Some(curve),
            });
            if args::optional_bool(args, "placeClip")?.unwrap_or(true) {
                let track = match args::optional_id(args, "playlistTrackId")? {
                    Some(track) => playlist_track(project, track)?,
                    None => {
                        let track = ids.take();
                        draft.expected.push(track);
                        draft.commands.push(Command::AddPlaylistTrack {
                            name: None,
                            index: None,
                        });
                        PlaylistTrackId(track)
                    }
                };
                draft.expected.push(ids.take());
                let length = (last - first).max(project.settings.time_signature.ticks_per_bar());
                draft.changes.push(format!(
                    "place it on playlist track {} from {}",
                    track.0,
                    grid.format_position(first)
                ));
                draft.commands.push(Command::AddClips {
                    clips: vec![ClipInit {
                        track,
                        start: first,
                        length: Some(length),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation {
                            automation: AutomationId(id),
                        },
                    }],
                });
            }
        }
    }
    propose(call, snapshot, args, draft)
}

/// `apply_raw_commands`: commands in their serialized form, for what the
/// other tools do not cover. The proposal registry still decides which
/// commands may be proposed.
pub fn raw_commands(call: &Call<'_>, snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &allowed(&["commands"]))?;
    let mut draft = Draft::new("Edit");
    for (index, value) in args::list(
        args,
        "commands",
        windfall_project::proposal::MAX_PROPOSAL_COMMANDS,
    )?
    .iter()
    .enumerate()
    {
        let command: Command = serde_json::from_value(value.clone())
            .map_err(|error| format!("command {index} is not a Windfall command: {error}"))?;
        draft.changes.push(format!(
            "command {index}: {}",
            value.get("type").and_then(Value::as_str).unwrap_or("?")
        ));
        draft.commands.push(command);
    }
    let check = CommandProposal {
        stamp: snapshot.stamp,
        label: "check".into(),
        explanation: String::new(),
        commands: draft.commands.clone(),
    };
    check.batch().map_err(|error| {
        format!("{error}; the commands a proposal may hold are listed in the tool's description")
    })?;
    propose(call, snapshot, args, draft)
}
