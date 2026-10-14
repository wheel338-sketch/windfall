//! The tools an agent can call: their names, descriptions and input
//! schemas, and the one function that runs them.

use std::time::Duration;

use serde_json::{Value, json};
use windfall_project::PatternId;

use crate::approvals::{Approvals, Approver, Entry, Status};
use crate::args::{self, Args};
use crate::host::{Host, RenderRegion, RenderRequest, Snapshot};
use crate::listen::{self, MAX_RENDER_SECONDS};
use crate::{edits, summary};

/// What a tool runs with.
pub struct Call<'a> {
    pub host: &'a dyn Host,
    pub approvals: &'a Approvals,
    /// The connected agent's name, from its client.
    pub agent: &'a str,
    /// True once the client has cancelled the request.
    pub cancelled: &'a dyn Fn() -> bool,
}

/// A tool's result: the structured value, and links to files it made.
pub struct Reply {
    pub value: Value,
    pub links: Vec<Value>,
}

impl From<Value> for Reply {
    fn from(value: Value) -> Self {
        Self {
            value,
            links: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Edit,
    Approval,
    Listen,
}

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub kind: Kind,
    pub description: &'static str,
    schema: fn() -> Value,
}

impl Tool {
    pub fn definition(&self) -> Value {
        let read_only = matches!(self.kind, Kind::Read | Kind::Listen);
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": (self.schema)(),
            "annotations": {
                "title": self.title,
                "readOnlyHint": read_only,
                "destructiveHint": false,
                "idempotentHint": read_only,
                "openWorldHint": false,
            },
        })
    }
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn id(description: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "maximum": u32::MAX, "description": description})
}

fn position(description: &str) -> Value {
    json!({
        "type": ["string", "integer"],
        "description": format!("{description} Write bar:beat:tick with bars and beats counted from 1, such as \"2:3:0\", or a number of ticks (960 a quarter note)."),
    })
}

fn length(description: &str) -> Value {
    json!({
        "type": ["string", "integer"],
        "description": format!("{description} A note value (\"1/4\", \"1/8.\" dotted, \"1/8t\" triplet, \"3/16\"), a duration bars:beats:ticks counted from 0 such as \"1:0:0\", or ticks."),
    })
}

fn pitch() -> Value {
    json!({"type": ["string", "integer"], "description": "A note name in scientific pitch (C4 is middle C, 60; sharps #, flats b) or a MIDI number 0 to 127."})
}

fn level() -> Value {
    json!({"type": ["number", "string"], "description": "A linear gain from 0 to 2 (1 is unity), or decibels as text such as \"-6 dB\"."})
}

fn page() -> [(&'static str, Value); 2] {
    [
        (
            "offset",
            json!({"type": "integer", "minimum": 0, "description": "Items to skip. The result's nextOffset continues a list."}),
        ),
        (
            "limit",
            json!({"type": "integer", "minimum": 1, "maximum": summary::MAX_PAGE, "description": "Items to return, 200 when left out."}),
        ),
    ]
}

/// The common `title` and `explanation` of edit tools, added to `own`.
fn edit(mut own: serde_json::Map<String, Value>, required: &[&str]) -> Value {
    own.insert(
        "title".into(),
        json!({"type": "string", "maxLength": 80, "description": "The name of the undo step, in the user's language. Windfall puts the agent's name in front."}),
    );
    own.insert(
        "explanation".into(),
        json!({"type": "string", "maxLength": 2000, "description": "Why, in the user's language; shown with the proposal in Windfall."}),
    );
    object(Value::Object(own), required)
}

fn props(items: Vec<(&str, Value)>) -> serde_json::Map<String, Value> {
    items
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn param_target() -> Vec<(&'static str, Value)> {
    vec![
        (
            "channelId",
            id("A channel: its instrument's settings, or volume, pan and muted."),
        ),
        (
            "trackId",
            id(
                "A mixer track: with effectId, that effect's settings (and mix, enabled); alone, the track's volume, pan, muted and its own EQ and stereo processing.",
            ),
        ),
        ("effectId", id("An effect on trackId.")),
    ]
}

/// Every tool, in the order `tools/list` gives them.
pub static TOOLS: &[Tool] = &[
    Tool {
        name: "get_project_summary",
        title: "Project summary",
        kind: Kind::Read,
        description: "The open project at a glance: tempo, meter, the key its notes suggest, channels with their instruments, patterns, playlist tracks, the mixer tracks in use with their effects and routing, automations and samples, with the ids every other tool takes. Read this first and again after edits.",
        schema: || object(json!({}), &[]),
    },
    Tool {
        name: "list_notes",
        title: "Notes in a pattern",
        kind: Kind::Read,
        description: "The notes of a pattern, optionally of one channel: id, pitch, start (bar:beat:tick in the pattern), length in ticks, velocity and pan. Paged.",
        schema: || {
            let mut items = props(vec![
                ("patternId", id("The pattern.")),
                ("channelId", id("Only this channel's notes.")),
            ]);
            items.extend(page().map(|(k, v)| (k.to_owned(), v)));
            object(Value::Object(items), &["patternId"])
        },
    },
    Tool {
        name: "list_clips",
        title: "Clips on the playlist",
        kind: Kind::Read,
        description: "The playlist clips that overlap a stretch of the song, optionally on one playlist track: id, track, start, length and what the clip plays. Paged.",
        schema: || {
            let mut items = props(vec![
                (
                    "from",
                    position("Start of the stretch; the song's start when left out."),
                ),
                (
                    "to",
                    position("End of the stretch; the song's end when left out."),
                ),
                ("trackId", id("Only clips on this playlist track.")),
            ]);
            items.extend(page().map(|(k, v)| (k.to_owned(), v)));
            object(Value::Object(items), &[])
        },
    },
    Tool {
        name: "get_mixer_track",
        title: "Mixer track and its chain",
        kind: Kind::Read,
        description: "One mixer track: level, pan, mute and solo, its effect chain in order with ids, its sends and sidechains, its output, and what plays into it.",
        schema: || {
            object(
                json!({"trackId": id("The mixer track; 0 is the master.")}),
                &["trackId"],
            )
        },
    },
    Tool {
        name: "get_parameters",
        title: "Settings of an instrument or effect",
        kind: Kind::Read,
        description: "The settings of a channel's instrument, of an effect, or of a mixer track's own processing: index, id, name, kind, unit, range, default and current value, and the choices of a choice. set_parameter and write_automation take the id or name. Paged; search filters by name.",
        schema: || {
            let mut items = props(param_target());
            items.insert("search".into(), json!({"type": "string", "description": "Only settings whose name or id contains this."}));
            items.extend(page().map(|(k, v)| (k.to_owned(), v)));
            object(Value::Object(items), &[])
        },
    },
    Tool {
        name: "get_selection",
        title: "What the user is working on",
        kind: Kind::Read,
        description: "The pattern the user is editing, the transport's mode, whether it plays or records, and the channel, mixer track and view the user last selected.",
        schema: || object(json!({}), &[]),
    },
    Tool {
        name: "list_instruments_and_effects",
        title: "Built-in instruments and effects",
        kind: Kind::Read,
        description: "Every built-in instrument and effect kind, with its name, how many settings it has and its first settings. add_channel and add_effect take the kind.",
        schema: || {
            object(
                json!({"category": {"type": "string", "enum": ["instrument", "effect"]}}),
                &[],
            )
        },
    },
    Tool {
        name: "create_pattern",
        title: "Create a pattern",
        kind: Kind::Edit,
        description: "Proposes a new pattern, optionally named, coloured and of a given length. The result says the id it will have.",
        schema: || {
            edit(
                props(vec![
                    ("name", json!({"type": "string", "maxLength": 128})),
                    (
                        "lengthSteps",
                        json!({"type": "integer", "minimum": 1, "maximum": 1024, "description": "Length in sixteenth-note steps."}),
                    ),
                    (
                        "lengthBars",
                        json!({"type": "integer", "minimum": 1, "maximum": 256, "description": "Length in bars of the project's meter."}),
                    ),
                    ("color", json!({"type": "string", "description": "#RRGGBB"})),
                ]),
                &[],
            )
        },
    },
    Tool {
        name: "add_notes",
        title: "Add notes",
        kind: Kind::Edit,
        description: "Proposes notes for one channel in one pattern. replace \"lane\" first removes the channel's notes in the pattern, \"overlap\" those starting inside the new notes' span. The pattern grows to whole bars to hold them unless extendPattern is false.",
        schema: || {
            edit(
                props(vec![
                    ("patternId", id("The pattern.")),
                    ("channelId", id("The channel that plays the notes.")),
                    (
                        "notes",
                        json!({"type": "array", "minItems": 1, "maxItems": 512, "items": object(json!({
                "pitch": pitch(),
                "start": position("Where the note starts in the pattern."),
                "length": length("How long it lasts; a sixteenth when left out."),
                "velocity": {"type": "number", "description": "0 to 1, or a MIDI velocity 2 to 127."},
                "pan": {"type": "number", "minimum": -1, "maximum": 1},
            }), &["pitch", "start"])}),
                    ),
                    (
                        "replace",
                        json!({"type": "string", "enum": ["none", "lane", "overlap"]}),
                    ),
                    ("extendPattern", json!({"type": "boolean"})),
                ]),
                &["patternId", "channelId", "notes"],
            )
        },
    },
    Tool {
        name: "remove_notes",
        title: "Remove notes",
        kind: Kind::Edit,
        description: "Proposes removing notes of one channel in one pattern: by id, or every note starting in from..to and, if given, of one pitch.",
        schema: || {
            edit(
                props(vec![
                    ("patternId", id("The pattern.")),
                    ("channelId", id("The channel.")),
                    (
                        "noteIds",
                        json!({"type": "array", "items": {"type": "integer", "minimum": 0}, "maxItems": 512}),
                    ),
                    ("from", position("First start included.")),
                    ("to", position("First start not included.")),
                    ("pitch", pitch()),
                ]),
                &["patternId", "channelId"],
            )
        },
    },
    Tool {
        name: "update_notes",
        title: "Change notes",
        kind: Kind::Edit,
        description: "Proposes changing notes by id: pitch or transpose (semitones), start, length, velocity, pan. Refused if a note changed since it was read.",
        schema: || {
            edit(
                props(vec![
                    ("patternId", id("The pattern.")),
                    ("channelId", id("The channel.")),
                    (
                        "updates",
                        json!({"type": "array", "minItems": 1, "maxItems": 512, "items": object(json!({
                "noteId": id("The note."),
                "pitch": pitch(),
                "transpose": {"type": "integer", "minimum": -127, "maximum": 127},
                "start": position("New start."),
                "length": length("New length."),
                "velocity": {"type": "number"},
                "pan": {"type": "number", "minimum": -1, "maximum": 1},
            }), &["noteId"])}),
                    ),
                ]),
                &["patternId", "channelId", "updates"],
            )
        },
    },
    Tool {
        name: "set_steps",
        title: "Set step-sequencer steps",
        kind: Kind::Edit,
        description: "Proposes step-sequencer steps for one channel, as text from firstStep on, one character a sixteenth: x on, X accented, . or - off, _ left as it is; spaces and | are ignored. A step turned on gets a one-step note of pitch (C5 when left out).",
        schema: || {
            edit(
                props(vec![
                    ("patternId", id("The pattern.")),
                    ("channelId", id("The channel.")),
                    (
                        "steps",
                        json!({"type": "string", "maxLength": 4096, "description": "Such as \"x...x...x...x...\"."}),
                    ),
                    (
                        "firstStep",
                        json!({"type": "integer", "minimum": 0, "maximum": 1023}),
                    ),
                    ("pitch", pitch()),
                    ("velocity", json!({"type": "number"})),
                    ("accentVelocity", json!({"type": "number"})),
                ]),
                &["patternId", "channelId", "steps"],
            )
        },
    },
    Tool {
        name: "add_channel",
        title: "Add a channel",
        kind: Kind::Edit,
        description: "Proposes a channel playing a built-in instrument, into a new mixer track of its own unless mixerTrackId is given.",
        schema: || {
            edit(
                props(vec![
                    (
                        "instrument",
                        json!({"type": "string", "description": "An instrument kind from list_instruments_and_effects, such as \"subtractiveSynth\"."}),
                    ),
                    ("name", json!({"type": "string", "maxLength": 128})),
                    (
                        "mixerTrackId",
                        id("Play into this mixer track instead of a new one."),
                    ),
                ]),
                &["instrument"],
            )
        },
    },
    Tool {
        name: "add_effect",
        title: "Add an effect",
        kind: Kind::Edit,
        description: "Proposes a built-in effect on a mixer track, at the end of its chain or at index. A track holds at most 10.",
        schema: || {
            edit(
                props(vec![
                    ("trackId", id("The mixer track.")),
                    (
                        "kind",
                        json!({"type": "string", "description": "An effect kind from list_instruments_and_effects, such as \"reverb\"."}),
                    ),
                    (
                        "index",
                        json!({"type": "integer", "minimum": 0, "maximum": 10}),
                    ),
                ]),
                &["trackId", "kind"],
            )
        },
    },
    Tool {
        name: "remove_effect",
        title: "Remove an effect",
        kind: Kind::Edit,
        description: "Proposes removing an effect, with its automation, from a mixer track.",
        schema: || {
            edit(
                props(vec![
                    ("trackId", id("The mixer track.")),
                    ("effectId", id("The effect.")),
                ]),
                &["trackId", "effectId"],
            )
        },
    },
    Tool {
        name: "move_effect",
        title: "Reorder an effect",
        kind: Kind::Edit,
        description: "Proposes moving an effect to index in its chain, or in toTrackId's chain. It keeps its id, settings and automation.",
        schema: || {
            edit(
                props(vec![
                    ("trackId", id("The mixer track it is on.")),
                    ("effectId", id("The effect.")),
                    (
                        "index",
                        json!({"type": "integer", "minimum": 0, "maximum": 10}),
                    ),
                    ("toTrackId", id("Another mixer track to move it to.")),
                ]),
                &["trackId", "effectId", "index"],
            )
        },
    },
    Tool {
        name: "set_parameter",
        title: "Set a setting",
        kind: Kind::Edit,
        description: "Proposes new values for settings of an instrument, an effect or a mixer track, by id or name from get_parameters. Values are checked against the setting's range: numbers in its unit, toggles as true or false, choices by label. Also volume, pan and muted of a channel or track, and mix and enabled of an effect.",
        schema: || {
            let mut items = props(param_target());
            items.insert("parameter".into(), json!({"type": ["string", "integer"], "description": "A setting's id, name or index."}));
            items.insert(
                "value".into(),
                json!({"type": ["number", "boolean", "string"]}),
            );
            items.insert("parameters".into(), json!({"type": "array", "maxItems": 64, "items": object(json!({
                "parameter": {"type": ["string", "integer"]},
                "value": {"type": ["number", "boolean", "string"]},
            }), &["parameter", "value"]), "description": "Several settings of the same target at once, instead of parameter and value."}));
            edit(items, &[])
        },
    },
    Tool {
        name: "route_channel",
        title: "Route a channel",
        kind: Kind::Edit,
        description: "Proposes sending a channel into another mixer track.",
        schema: || {
            edit(
                props(vec![
                    ("channelId", id("The channel.")),
                    ("trackId", id("The mixer track it plays into.")),
                ]),
                &["channelId", "trackId"],
            )
        },
    },
    Tool {
        name: "route_track",
        title: "Route a mixer track",
        kind: Kind::Edit,
        description: "Proposes a mixer track's output. Routing that loops back on itself is refused.",
        schema: || {
            edit(
                props(vec![
                    ("trackId", id("The mixer track.")),
                    (
                        "outputId",
                        id("The mixer track it goes to; 0 is the master."),
                    ),
                ]),
                &["trackId", "outputId"],
            )
        },
    },
    Tool {
        name: "set_send",
        title: "Set a send",
        kind: Kind::Edit,
        description: "Proposes a send from one mixer track to another at a level, or removing it.",
        schema: || {
            edit(
                props(vec![
                    ("fromTrackId", id("The track that sends.")),
                    ("toTrackId", id("The track that receives.")),
                    ("level", level()),
                    ("remove", json!({"type": "boolean"})),
                ]),
                &["fromTrackId", "toTrackId"],
            )
        },
    },
    Tool {
        name: "place_clips",
        title: "Place clips",
        kind: Kind::Edit,
        description: "Proposes pattern or automation clips on the playlist. Clips without trackId go on one new playlist track. A pattern clip's length defaults to one pass of the pattern.",
        schema: || {
            edit(
                props(vec![
                    (
                        "clips",
                        json!({"type": "array", "minItems": 1, "maxItems": 128, "items": object(json!({
                "patternId": id("The pattern the clip plays."),
                "automationId": id("Or the automation it shows."),
                "trackId": id("The playlist track."),
                "start": position("Where the clip starts in the song."),
                "length": length("How long the clip is."),
                "offset": length("How far into its content the clip starts."),
                "muted": {"type": "boolean"},
            }), &["start"])}),
                    ),
                    ("newTrackName", json!({"type": "string", "maxLength": 128})),
                ]),
                &["clips"],
            )
        },
    },
    Tool {
        name: "move_clips",
        title: "Move clips",
        kind: Kind::Edit,
        description: "Proposes new places, lengths, offsets or mutes for clips by id.",
        schema: || {
            edit(
                props(vec![(
                    "clips",
                    json!({"type": "array", "minItems": 1, "maxItems": 128, "items": object(json!({
                "clipId": id("The clip."),
                "trackId": id("Another playlist track."),
                "start": position("New start."),
                "length": length("New length."),
                "offset": length("New offset into its content."),
                "muted": {"type": "boolean"},
            }), &["clipId"])}),
                )]),
                &["clips"],
            )
        },
    },
    Tool {
        name: "remove_clips",
        title: "Remove clips",
        kind: Kind::Edit,
        description: "Proposes removing clips from the playlist. What they play stays in the project.",
        schema: || {
            edit(
                props(vec![(
                    "clipIds",
                    json!({"type": "array", "minItems": 1, "maxItems": 128, "items": {"type": "integer", "minimum": 0}}),
                )]),
                &["clipIds"],
            )
        },
    },
    Tool {
        name: "set_tempo_meter",
        title: "Set tempo and meter",
        kind: Kind::Edit,
        description: "Proposes the project's tempo (10 to 522 bpm), its meter (such as \"3/4\"), or both.",
        schema: || {
            edit(
                props(vec![
                    (
                        "tempoBpm",
                        json!({"type": "number", "minimum": 10, "maximum": 522}),
                    ),
                    (
                        "meter",
                        json!({"type": "string", "description": "Beats/unit: 1 to 16 over 2, 4, 8 or 16."}),
                    ),
                ]),
                &[],
            )
        },
    },
    Tool {
        name: "rename_and_color",
        title: "Rename or colour",
        kind: Kind::Edit,
        description: "Proposes a new name, colour or both for a channel, pattern, mixer track or playlist track.",
        schema: || {
            edit(
                props(vec![
                    (
                        "target",
                        json!({"type": "string", "enum": ["channel", "pattern", "mixerTrack", "playlistTrack"]}),
                    ),
                    ("id", id("Its id.")),
                    ("name", json!({"type": "string", "maxLength": 128})),
                    ("color", json!({"type": "string", "description": "#RRGGBB"})),
                ]),
                &["target", "id"],
            )
        },
    },
    Tool {
        name: "write_automation",
        title: "Write automation",
        kind: Kind::Edit,
        description: "Proposes an automation curve at song positions, in the target's own values: a channel's or track's volume (level) or pan, a send's level, an effect's mix or setting, an instrument's setting, a track's processing setting, or the tempo. A new automation is put on a new playlist track unless playlistTrackId is given or placeClip is false. With automationId its points are replaced instead.",
        schema: || {
            edit(
                props(vec![
                    (
                        "target",
                        object(
                            json!({
                                "channelId": id("A channel."),
                                "trackId": id("A mixer track."),
                                "effectId": id("An effect on trackId."),
                                "sendTo": id("With trackId and parameter \"level\": the send to this track."),
                                "parameter": {"type": ["string", "integer"], "description": "\"volume\", \"pan\", \"mix\", \"level\", \"tempo\", or a setting's id or name."},
                            }),
                            &["parameter"],
                        ),
                    ),
                    (
                        "points",
                        json!({"type": "array", "minItems": 1, "maxItems": 1024, "items": object(json!({
                "at": position("Where in the song."),
                "value": {"type": ["number", "boolean", "string"], "description": "In the target's own terms, as set_parameter takes them."},
                "curve": {"type": "number", "minimum": -1, "maximum": 1},
                "hold": {"type": "boolean"},
            }), &["at", "value"])}),
                    ),
                    (
                        "automationId",
                        id("An automation of the same target whose points to replace."),
                    ),
                    ("name", json!({"type": "string", "maxLength": 128})),
                    (
                        "playlistTrackId",
                        id("The playlist track for the new automation's clip."),
                    ),
                    ("placeClip", json!({"type": "boolean"})),
                ]),
                &["target", "points"],
            )
        },
    },
    Tool {
        name: "apply_raw_commands",
        title: "Propose raw commands",
        kind: Kind::Edit,
        description: "Proposes Windfall commands in their serialized form, for what the other tools do not cover: JSON objects with a camelCase \"type\" and camelCase fields, ids as numbers and times in ticks, such as {\"type\": \"updateChannel\", \"id\": 4, \"patch\": {\"pan\": -0.25}} or {\"type\": \"addMixerTrack\", \"name\": \"Bus\"}. A proposal may hold: addPattern, updatePattern, addChannel, updateChannel, addNotes, removeNotes, updateCapturedNotes, addMixerTrack, updateMixerTrack, setTrackOutput, setSend, setSidechain, addEffect, removeEffect, moveEffect, updateEffect, setInstrumentParam, setEffectParam, setTrackParam, setPluginParam, addPlaylistTrack, updatePlaylistTrack, addClips, updateClips, removeClips, addAutomation, setAutomationPoints, setScriptLibrary, and updateSettings of tempoBpm and timeSignature only. Never files, project replacement or nested batches.",
        schema: || {
            edit(
                props(vec![(
                    "commands",
                    json!({"type": "array", "minItems": 1, "maxItems": 128, "items": {"type": "object", "required": ["type"]}}),
                )]),
                &["commands"],
            )
        },
    },
    Tool {
        name: "get_proposal",
        title: "How a proposal stands",
        kind: Kind::Approval,
        description: "Whether a proposal is pending, applied, rejected, withdrawn or failed, and whether the project moved on so it went stale. waitSeconds waits up to 60 seconds for the user's decision.",
        schema: || {
            object(
                json!({
                    "proposalId": {"type": "string"},
                    "waitSeconds": {"type": "integer", "minimum": 0, "maximum": 60},
                }),
                &["proposalId"],
            )
        },
    },
    Tool {
        name: "apply_proposal",
        title: "Apply a proposal",
        kind: Kind::Approval,
        description: "Applies one of this agent's pending proposals, only when the user has allowed this agent to apply without asking. Otherwise the user applies it in Windfall.",
        schema: || object(json!({"proposalId": {"type": "string"}}), &["proposalId"]),
    },
    Tool {
        name: "withdraw_proposal",
        title: "Withdraw a proposal",
        kind: Kind::Approval,
        description: "Takes back one of this agent's pending proposals.",
        schema: || object(json!({"proposalId": {"type": "string"}}), &["proposalId"]),
    },
    Tool {
        name: "render_and_measure",
        title: "Listen: render and measure",
        kind: Kind::Listen,
        description: "Renders offline, without playing, and measures: integrated loudness (LUFS), loudness range, true and sample peak, clipping, RMS over time, stereo correlation, energy per octave band, detected tempo and key. Also keeps the render as a WAV file in a Windfall folder and returns its path. region: \"song\" (the default), {\"from\", \"to\"} song positions, or {\"patternId\", \"loops\"}. soloTrackId renders one mixer track alone. At most 120 seconds.",
        schema: || {
            object(
                json!({
                    "region": {"description": "\"song\", {\"from\": position, \"to\": position}, or {\"patternId\": id, \"loops\": 1-16}."},
                    "soloTrackId": id("Render only this mixer track, as its solo button does."),
                    "maxSeconds": {"type": "number", "minimum": 1, "maximum": MAX_RENDER_SECONDS, "description": "Refuse regions longer than this; 60 when left out."},
                }),
                &[],
            )
        },
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|tool| tool.name == name)
}

/// Runs a tool. `Err` is a tool execution error: the agent is told what
/// was wrong and can correct the call.
pub fn run(tool: &Tool, call: &Call<'_>, arguments: Option<&Value>) -> Result<Reply, String> {
    let args = args::object(arguments)?;
    call.approvals.seen(call.agent);
    match tool.kind {
        Kind::Approval => return approval(tool.name, call, args).map(Reply::from),
        Kind::Listen => return listen_tool(call, args),
        Kind::Read | Kind::Edit => {}
    }
    let snapshot = call.host.snapshot()?;
    let value = match tool.name {
        "get_project_summary" => {
            args::known(args, &[])?;
            summary::project_summary(&snapshot)
        }
        "list_notes" => summary::list_notes(&snapshot, args)?,
        "list_clips" => summary::list_clips(&snapshot, args)?,
        "get_mixer_track" => summary::mixer_track(&snapshot, args)?,
        "get_parameters" => summary::parameters(&snapshot, args)?,
        "get_selection" => {
            args::known(args, &[])?;
            summary::selection(&snapshot, &call.host.focus())
        }
        "list_instruments_and_effects" => summary::kinds(args)?,
        "create_pattern" => edits::create_pattern(call, &snapshot, args)?,
        "add_notes" => edits::add_notes(call, &snapshot, args)?,
        "remove_notes" => edits::remove_notes(call, &snapshot, args)?,
        "update_notes" => edits::update_notes(call, &snapshot, args)?,
        "set_steps" => edits::set_steps(call, &snapshot, args)?,
        "add_channel" => edits::add_channel(call, &snapshot, args)?,
        "add_effect" => edits::add_effect(call, &snapshot, args)?,
        "remove_effect" => edits::remove_effect(call, &snapshot, args)?,
        "move_effect" => edits::move_effect(call, &snapshot, args)?,
        "set_parameter" => edits::set_parameter(call, &snapshot, args)?,
        "route_channel" => edits::route_channel(call, &snapshot, args)?,
        "route_track" => edits::route_track(call, &snapshot, args)?,
        "set_send" => edits::set_send(call, &snapshot, args)?,
        "place_clips" => edits::place_clips(call, &snapshot, args)?,
        "move_clips" => edits::move_clips(call, &snapshot, args)?,
        "remove_clips" => edits::remove_clips(call, &snapshot, args)?,
        "set_tempo_meter" => edits::set_tempo_meter(call, &snapshot, args)?,
        "rename_and_color" => edits::rename_and_color(call, &snapshot, args)?,
        "write_automation" => edits::write_automation(call, &snapshot, args)?,
        "apply_raw_commands" => edits::raw_commands(call, &snapshot, args)?,
        _ => unreachable!("every listed tool has a case"),
    };
    Ok(Reply::from(value))
}

/// How a proposal looks to the agent that made it.
pub fn describe(entry: &Entry, current: Option<&Snapshot>) -> Value {
    let stale = entry.status == Status::Pending
        && current.is_some_and(|snapshot| snapshot.stamp != entry.proposal.stamp);
    json!({
        "proposalId": entry.id,
        "status": entry.status,
        "stale": stale,
        "label": entry.proposal.label,
        "automatic": entry.automatic,
        "changes": entry.preview.changes,
        "created": entry.preview.created,
        "counts": entry.preview.counts,
        "stamp": entry.proposal.stamp,
        "waitedSeconds": entry.filed.elapsed().as_secs(),
    })
}

fn own<'a>(call: &Call<'_>, args: &'a Args) -> Result<(&'a str, Entry), String> {
    let id = args::text(args, "proposalId", 32)?;
    let entry = call
        .approvals
        .get(id)
        .ok_or_else(|| format!("there is no proposal {id}"))?;
    if entry.agent != call.agent {
        return Err(format!("proposal {id} belongs to another agent"));
    }
    Ok((id, entry))
}

fn approval(name: &str, call: &Call<'_>, args: &Args) -> Result<Value, String> {
    match name {
        "get_proposal" => {
            args::known(args, &["proposalId", "waitSeconds"])?;
            let (id, mut entry) = own(call, args)?;
            if let Some(wait) = args::optional_u32(args, "waitSeconds", 0..=60)?
                && wait > 0
            {
                // Waits in short slices, so a cancelled request ends soon.
                let deadline = std::time::Instant::now() + Duration::from_secs(u64::from(wait));
                while !entry.status.decided() && !(call.cancelled)() {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    entry = call
                        .approvals
                        .wait(id, left.min(Duration::from_millis(250)))
                        .ok_or_else(|| format!("proposal {id} is gone"))?;
                }
            }
            let snapshot = call.host.snapshot().ok();
            Ok(describe(&entry, snapshot.as_ref()))
        }
        "apply_proposal" => {
            args::known(args, &["proposalId"])?;
            let (id, _) = own(call, args)?;
            call.approvals
                .apply(call.host, id, Approver::Agent(call.agent))?;
            let entry = call
                .approvals
                .get(id)
                .ok_or_else(|| format!("proposal {id} is gone"))?;
            Ok(describe(&entry, None))
        }
        "withdraw_proposal" => {
            args::known(args, &["proposalId"])?;
            let (id, _) = own(call, args)?;
            call.approvals.withdraw(id, call.agent)?;
            call.host.proposals_changed();
            let entry = call
                .approvals
                .get(id)
                .ok_or_else(|| format!("proposal {id} is gone"))?;
            Ok(describe(&entry, None))
        }
        _ => unreachable!("every approval tool has a case"),
    }
}

fn listen_tool(call: &Call<'_>, args: &Args) -> Result<Reply, String> {
    args::known(args, &["region", "soloTrackId", "maxSeconds"])?;
    let snapshot = call.host.snapshot()?;
    let project = snapshot.document.project();
    let grid = summary::song_grid(project);
    let region = match args::optional_value(args, "region") {
        None => RenderRegion::Song,
        Some(Value::String(text)) if text == "song" => RenderRegion::Song,
        Some(Value::Object(region)) => {
            if region.contains_key("patternId") {
                args::known(region, &["patternId", "loops"])?;
                let pattern = summary::pattern_of(project, args::id(region, "patternId")?)?;
                RenderRegion::Pattern {
                    pattern: PatternId(pattern.id.0),
                    loops: args::optional_u32(region, "loops", 1..=16)?.unwrap_or(1),
                }
            } else {
                args::known(region, &["from", "to"])?;
                let start = grid.position_value(args::value(region, "from")?)?;
                let end = grid.position_value(args::value(region, "to")?)?;
                if end <= start {
                    return Err("region to must come after from".into());
                }
                RenderRegion::Range { start, end }
            }
        }
        Some(other) => {
            return Err(format!(
                "region {other} must be \"song\", {{\"from\", \"to\"}} or {{\"patternId\", \"loops\"}}"
            ));
        }
    };
    if region == RenderRegion::Song && project.playlist.clips.is_empty() {
        return Err(
            "the playlist is empty, so there is no song to render; render a pattern instead".into(),
        );
    }
    let solo = args::optional_id(args, "soloTrackId")?
        .map(|id| summary::track_of(project, id).map(|track| track.id))
        .transpose()?;
    let max_seconds = args::optional_number(args, "maxSeconds")?.unwrap_or(60.0);
    if !(1.0..=MAX_RENDER_SECONDS).contains(&max_seconds) {
        return Err(format!("maxSeconds must be from 1 to {MAX_RENDER_SECONDS}"));
    }
    let rendered = call.host.render(
        RenderRequest {
            region,
            solo,
            max_seconds,
        },
        call.cancelled,
    )?;
    let measurements = listen::measure(&rendered.samples, rendered.sample_rate);
    let mut links = Vec::new();
    if let Some(path) = &rendered.path {
        links.push(json!({
            "type": "resource_link",
            "uri": file_uri(path),
            "name": "render.wav",
            "description": "The render, 32-bit float WAV.",
            "mimeType": "audio/wav",
        }));
    }
    Ok(Reply {
        value: json!({
            "measurements": measurements,
            "wavPath": rendered.path,
            "projectTempoBpm": project.settings.tempo_bpm,
            "stamp": snapshot.stamp,
        }),
        links,
    })
}

/// A `file:` URI for a local path, with the characters a URI cannot hold
/// escaped.
pub fn file_uri(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut uri = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                uri.push(byte as char)
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}
