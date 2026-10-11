//! Bringing a [`MidiSong`] into a project.
//!
//! [`import`] works out what a song becomes in a project and returns it as
//! an [`ImportPlan`]: the channels, patterns and clips it will make, in
//! terms a dialog can show, and a list of everything that had to be fitted
//! or left behind. The plan knows nothing of any project. Once the user has
//! agreed to it, [`ImportPlan::command`] turns it into one command for the
//! project that is open, and dispatching that command is one undo step.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use windfall_project::{
    AutomationId, AutomationPoint, AutomationRange, AutomationTarget, ChannelId, ChannelPatch,
    ClipContent, ClipInit, Command, DEFAULT_KEY, InstrumentKind, MAX_AUTOMATION_POINTS,
    MAX_MIXER_SIGNAL_TRACKS, MAX_PATTERN_TICKS, MAX_SONG_TICKS, MAX_TEMPO_BPM, MIN_TEMPO_BPM,
    MarkerKind, MeterChange, MeterChangeId, NoteInit, PatternId, PatternPatch, PatternTimelineEdit,
    PlaylistTrackId, Project, SampleId, SamplePath, SettingsPatch, TICKS_PER_STEP, TimeSignature,
    Timeline, TimelineMarker, TimelineMarkerId,
};

use crate::expression::{pressure_from_midi, timbre_from_midi};
use crate::gm::{DrumKit, program_name};
use crate::import_expression::{self, NoteValues};
use crate::mix::{pan_from_midi, velocity_from_midi, volume_from_midi};
use crate::song::{
    CC_PAN, CC_VOLUME, ControlKind, DEFAULT_MICROS_PER_QUARTER, DRUM_CHANNEL, MidiSong, MidiTrack,
    bpm,
};

/// The most notes one import lays out on the playlist. A note that is
/// split across patterns counts once for each piece, and a note of a
/// pattern that several clips share counts once for each clip. Notes past
/// the limit are left out and reported.
pub const MAX_IMPORTED_NOTES: usize = 1_000_000;

/// The longest name an imported channel, pattern or playlist track gets,
/// in characters.
const MAX_NAME_CHARS: usize = 64;

/// The history label of an import.
const LABEL: &str = "Import MIDI";

/// The time signature of a MIDI file that sets none.
const COMMON_TIME: TimeSignature = TimeSignature {
    numerator: 4,
    denominator: 4,
};

/// Choices for arranging a MIDI song and applying its settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportOptions {
    pub patterns: PatternStrategy,
    /// Plays the MIDI drum channel with samples: each drum the file uses
    /// and the kit has a sound for gets a sampler channel of its own.
    /// `None` leaves the drum channel on a synth channel, where its keys
    /// are heard as pitches. [`DrumKit::factory`] is a kit of the sounds
    /// Windfall ships with.
    pub drum_kit: Option<DrumKit>,
    /// Sets the project's tempo to the file's first, and follows the
    /// file's tempo changes with a tempo automation.
    pub tempo: bool,
    /// Sets the scalar signature from the effective tick-zero meter and
    /// imports the ordered song meter changes.
    pub time_signature: bool,
    /// Sets each channel's volume and pan from the first channel volume
    /// (controller 7) and pan (controller 10) its MIDI channel is given.
    pub channel_mix: bool,
    /// Gives each note the fine pitch and modulation its channel has when
    /// it starts: pitch bend scaled by the channel's bend range, controller
    /// 74 as modulation X and the modulation wheel as modulation Y. Off,
    /// every bend and controller is left behind and reported.
    pub note_expression: bool,
}

impl Default for ImportOptions {
    /// One pattern for each track, no drum kit, and the file's tempo, time
    /// signature, volumes, pans and note expression.
    fn default() -> Self {
        Self {
            patterns: PatternStrategy::default(),
            drum_kit: None,
            tempo: true,
            time_signature: true,
            channel_mix: true,
            note_expression: true,
        }
    }
}

/// How the notes of a part are divided into patterns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PatternStrategy {
    /// One pattern that spans the song. A pattern holds at most 1,024
    /// steps, which is 64 bars of 4/4, so a longer song gets one pattern
    /// for each stretch of that length, cut on a bar line.
    #[default]
    PerTrack,
    /// A pattern for every `bars` bars, as a song built in the channel
    /// rack would have. Stretches in which a part plays nothing get no
    /// pattern.
    Bars {
        /// Bars to a pattern, at least 1. More than a pattern can hold
        /// means as many as it can.
        bars: u32,
        /// Lets stretches of a part that hold exactly the same notes use
        /// one pattern, placed once for each of them, so that a verse
        /// played three times is one pattern and three clips.
        share: bool,
    },
}

/// What importing a song will do to a project.
///
/// Every index in the plan points into another list of the plan. Nothing in
/// it is an id of a project: [`command`](Self::command) works those out for
/// the project it is given.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportPlan {
    /// The song's name, if the file has one. It is offered to the caller,
    /// which may name a new project after it; the import leaves the
    /// project's name alone.
    pub name: Option<String>,
    /// The tempo the project will be set to. `None` leaves it as it is.
    pub tempo_bpm: Option<f64>,
    /// The time signature the project will be set to. `None` leaves it as
    /// it is.
    pub time_signature: Option<TimeSignature>,
    /// Ordered song meter events. Importing replaces the prior song map.
    pub meters: Vec<(u32, TimeSignature)>,
    pub markers: Vec<(u32, String)>,
    /// The imported clip layout's length, rounded to whole bars of the
    /// tick-zero signature (4/4 before a late first event). Later meter
    /// changes keep their absolute ticks; pattern cutting uses this scalar grid.
    pub length: u32,
    /// The samples the project will be given, each path once. Only a drum
    /// kit brings any.
    pub samples: Vec<PlannedSample>,
    /// The channels that will be added to the rack, in order.
    pub channels: Vec<PlannedChannel>,
    /// The names of the playlist tracks that will be added, one for each
    /// part: a MIDI channel of a MIDI track that has notes.
    pub playlist_tracks: Vec<String>,
    pub patterns: Vec<PlannedPattern>,
    /// Where the patterns go on the playlist.
    pub clips: Vec<PlannedClip>,
    /// The curve of the tempo automation that will be added, every point
    /// a step. Empty when the tempo never changes, in which case there is
    /// no automation.
    pub tempo_points: Vec<AutomationPoint>,
    /// Everything that was fitted to what a project can hold, and
    /// everything the file says that a project has no place for.
    pub adjustments: Vec<Adjustment>,
}

/// A sample to register before creating its sampler channel.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSample {
    pub name: String,
    pub path: SamplePath,
}

/// A rack channel and the MIDI part it represents.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedChannel {
    pub name: String,
    pub sound: PlannedSound,
    /// The track of the song the channel's notes come from, counted from
    /// 0, and the MIDI channel they were on.
    pub midi_track: usize,
    pub midi_channel: u8,
    /// Whether that is the MIDI drum channel.
    pub drums: bool,
    /// The General MIDI program the file asks for there, which Windfall
    /// has no instrument to match yet. [`program_name`] names it.
    pub program: Option<u8>,
    /// The volume and pan the channel will be given. `None` leaves the
    /// default of a new channel.
    pub volume: Option<f32>,
    pub pan: Option<f32>,
    /// How many notes the channel gets in the patterns. A pattern that is
    /// placed several times holds its notes once.
    pub notes: usize,
}

/// What an imported channel plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedSound {
    /// The subtractive synth at its default settings.
    Synth,
    /// A sampler with the sample at this index of [`ImportPlan::samples`].
    Sample(usize),
}

/// A pattern to create, with lanes referring to planned channels.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPattern {
    pub name: String,
    pub length_steps: u32,
    pub time_signature: Option<TimeSignature>,
    pub meters: Vec<(u32, TimeSignature)>,
    pub markers: Vec<(u32, String)>,
    /// The notes of each channel that plays in the pattern.
    pub lanes: Vec<PlannedLane>,
}

/// Notes for one planned channel in one pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedLane {
    /// Index into [`ImportPlan::channels`].
    pub channel: usize,
    pub notes: Vec<NoteInit>,
}

/// A placement of a planned pattern on a planned playlist track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedClip {
    /// Index into [`ImportPlan::patterns`].
    pub pattern: usize,
    /// Index into [`ImportPlan::playlist_tracks`].
    pub track: usize,
    pub start: u32,
    pub length: u32,
}

/// One thing an import changed or left behind, for the user to read before
/// agreeing to it. `to_string` gives a sentence.
#[derive(Debug, Clone, PartialEq)]
pub enum Adjustment {
    /// Notes that start past the end of the longest song were left out.
    NotesPastEnd { count: usize },
    /// Notes that ran past the end of the longest song were shortened to
    /// end there.
    NotesCutAtEnd { count: usize },
    /// Notes that last past the end of their pattern were split there:
    /// the rest of each is a new note at the start of the next pattern,
    /// which strikes the key again.
    NotesSplit { count: usize },
    /// Notes beyond [`MAX_IMPORTED_NOTES`] were left out.
    NotesOverLimit { count: usize },
    /// Tempos outside 10 to 522 bpm were brought to the nearer end.
    TempoOutOfRange { count: usize },
    /// The file has more tempo changes than an automation has points:
    /// `kept` of its `changes` were kept, evenly spread.
    TempoChangesThinned { changes: usize, kept: usize },
    /// The file's time signature is one a project cannot have, and `used`
    /// is the nearest it can.
    TimeSignatureFitted {
        numerator: u8,
        denominator: u8,
        used: TimeSignature,
    },
    /// The time signature changes during the song. A project has one, so
    /// the first is used and bar lines after the first change are off.
    TimeSignatureChanges { count: usize },
    /// A part on the MIDI drum channel plays a synth, for want of a drum
    /// kit: every key is heard as a pitch and not as the drum it stands
    /// for.
    DrumsAsSynth { channel: String },
    /// Drum notes on keys the drum kit has no sound for stay on a synth
    /// channel.
    DrumKeysWithoutPad { notes: usize },
    /// Something the file says that a project has no place for yet.
    NotImported { what: Unsupported, count: usize },
    /// Notes that take a fine pitch or a modulation from the pitch bend and
    /// controllers their channel has when they start.
    ExpressionImported { notes: usize },
    /// Notes whose pitch bend reaches further than the octave either way
    /// that a note's fine pitch holds.
    FinePitchFitted { notes: usize },
}

/// A kind of thing in a MIDI file that an import leaves behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    PitchBend,
    /// Controller messages, apart from the first volume and pan of each
    /// part when those are imported.
    Controllers,
    Aftertouch,
    /// Program changes. Every melodic channel plays the same synth, named
    /// after the program where the track has no name.
    ProgramChanges,
    KeySignatures,
    Markers,
}

impl fmt::Display for Adjustment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let notes = |count| counted(count, "note", "notes");
        match self {
            Adjustment::NotesPastEnd { count } => write!(
                f,
                "{} left out: they start past the end of the longest song Windfall holds",
                notes(*count)
            ),
            Adjustment::NotesCutAtEnd { count } => write!(
                f,
                "{} shortened to end with the longest song Windfall holds",
                notes(*count)
            ),
            Adjustment::NotesSplit { count } => write!(
                f,
                "{} split where a pattern ends: the rest of each starts the next pattern as a new note",
                notes(*count)
            ),
            Adjustment::NotesOverLimit { count } => write!(
                f,
                "{} left out: one import brings in at most {MAX_IMPORTED_NOTES} notes",
                notes(*count)
            ),
            Adjustment::TempoOutOfRange { count } => write!(
                f,
                "{} brought into Windfall's range of {MIN_TEMPO_BPM} to {MAX_TEMPO_BPM} bpm",
                counted(*count, "tempo", "tempos")
            ),
            Adjustment::TempoChangesThinned { changes, kept } => write!(
                f,
                "the tempo changes {changes} times and {kept} of the changes are kept, which is as many as an automation holds"
            ),
            Adjustment::TimeSignatureFitted {
                numerator,
                denominator,
                used,
            } => write!(
                f,
                "the time signature {numerator}/{denominator} is not one Windfall has, so {}/{} is used",
                used.numerator, used.denominator
            ),
            Adjustment::TimeSignatureChanges { count } => write!(
                f,
                "{} not imported: colliding events, song bounds or the timeline item limit",
                counted(*count, "time signature change", "time signature changes")
            ),
            Adjustment::DrumsAsSynth { channel } => write!(
                f,
                "\"{channel}\" is on the MIDI drum channel and plays a synth, so its drums are heard as pitches"
            ),
            Adjustment::DrumKeysWithoutPad { notes: count } => write!(
                f,
                "{} on keys the drum kit has no sound for, left on a synth channel",
                counted(*count, "drum note", "drum notes")
            ),
            Adjustment::NotImported { what, count } => {
                let (one, many) = match what {
                    Unsupported::PitchBend => ("pitch bend message", "pitch bend messages"),
                    Unsupported::Controllers => ("controller message", "controller messages"),
                    Unsupported::Aftertouch => ("aftertouch message", "aftertouch messages"),
                    Unsupported::ProgramChanges => ("program change", "program changes"),
                    Unsupported::KeySignatures => ("key signature", "key signatures"),
                    Unsupported::Markers => ("marker", "markers"),
                };
                write!(f, "{} not imported", counted(*count, one, many))
            }
            Adjustment::ExpressionImported { notes: count } => write!(
                f,
                "{} given the fine pitch and modulation of the pitch bend and controllers at their start; later movement inside a note is not imported",
                notes(*count)
            ),
            Adjustment::FinePitchFitted { notes: count } => write!(
                f,
                "{} bent further than an octave, which is as far as a note's fine pitch goes",
                notes(*count)
            ),
        }
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// Works out what importing `song` into a project will do.
///
/// # What a song becomes
///
/// - **Channels.** Each MIDI channel of each track that has notes is a
///   *part*, and becomes one channel that plays the subtractive synth, with
///   a mixer track of its own as every new channel has. A track that plays
///   on one channel gives its name to it: the track's name, else its
///   instrument name, else the General MIDI name of its first program, else
///   "Track 3". The channels of a track that plays on several, as the one
///   track of a format 0 file does, are named after their programs, or
///   "Track 1 ch 2" for want of one. The MIDI drum channel is "Drums"
///   unless its track has a name to itself. A note's velocity of 1 to 127
///   becomes `velocity / 127`.
/// - **Drums.** With [`ImportOptions::drum_kit`], each of the kit's samples
///   that a drum part plays becomes a sampler channel, and the notes of
///   its keys move there, on the key that plays the sample as recorded.
///   Without a kit the part is a synth channel like any other, and the
///   plan says so.
/// - **Patterns and clips.** Each part gets a playlist track and the
///   patterns [`ImportOptions::patterns`] asks for, placed where their
///   notes belong. Lengths are whole bars of the song's first time
///   signature, 4/4 if it has none, and the song is as long as the file,
///   rounded up to a bar.
/// - **Tempo.** The project's tempo becomes that of the file's first tempo
///   event. If the tempo changes, a tempo automation steps through the
///   changes on a playlist track called "Tempo", in a clip as long as the
///   song. A file whose first tempo event is not at the start begins at
///   MIDI's default of 120 bpm, and so does the automation. A file with no
///   tempo event leaves the project's tempo alone, and a tempo event where
///   the song ends, with nothing left to play, is passed over. A tempo
///   is stored in an automation as a 32-bit fraction of the range, which
///   can move it by a millionth of itself.
/// - **Time signature.** The file's first, if it has one.
/// - **Volume and pan.** The first channel volume and pan of each part.
///   A volume of `v` becomes `0.8 * (v / 100)²`, and a pan of 0, 64 and
///   127 hard left, center and hard right.
///
/// # What is fitted
///
/// Everything here is listed in [`ImportPlan::adjustments`].
///
/// - A note that lasts past the end of its pattern is split there, because
///   a clip ends the notes of its pattern where it ends. The rest of the
///   note starts the next pattern as a new note.
/// - A song is at most [`MAX_SONG_TICKS`] long. Notes that start past that
///   are left out and notes that run past it are shortened.
/// - A tempo outside 10 to 522 bpm becomes the nearer of the two, and of
///   more than 4,096 tempo changes, 4,096 are kept, evenly spread.
/// - A time signature with more than 16 beats to the bar gets 16, and a
///   beat unit of a whole note becomes a half note and anything shorter
///   than a sixteenth note a sixteenth.
/// - Pitch bend, controllers other than the first volume and pan,
///   aftertouch, program changes and key signatures are not imported.
///   The [`MidiSong`] still holds them. Ordered meter changes are imported,
///   with colliding and over-limit events reported.
///
/// Two more things change without a line in the list. Names are cut to 64
/// characters and lose their control characters. And a `MidiSong` that is
/// not in its normal form is imported as its normal form.
pub fn import(song: &MidiSong, options: &ImportOptions) -> ImportPlan {
    let song = song.clone().normalized();
    // A late first event does not change the signature before it. Export
    // explicitly writes that initial meter, so sizing must use the same
    // authority before and after a canonical file round trip. Of several
    // tick-zero events, the last is the one the checked meter map retains.
    let first = song
        .time_signatures
        .iter()
        .take_while(|event| event.tick == 0)
        .last();
    let signature = first.map(|first| fit_signature(first.numerator, first.denominator));
    let bar = signature.unwrap_or(COMMON_TIME).ticks_per_bar();
    // The longest song and the longest pattern that are whole bars.
    let limit = MAX_SONG_TICKS / bar * bar;
    let longest = MAX_PATTERN_TICKS / bar * bar;
    let length = song.end().min(limit).div_ceil(bar).max(1) * bar;
    let (segment, share) = match options.patterns {
        PatternStrategy::PerTrack => (longest, false),
        PatternStrategy::Bars { bars, share } => {
            let ticks = u64::from(bars.max(1)) * u64::from(bar);
            (u32::try_from(ticks).unwrap_or(u32::MAX).min(longest), share)
        }
    };

    let mut builder = Builder {
        options,
        plan: ImportPlan {
            name: song.name.as_deref().and_then(clean_name),
            tempo_bpm: None,
            time_signature: signature.filter(|_| options.time_signature),
            meters: Vec::new(),
            markers: song
                .markers
                .iter()
                .filter(|marker| marker.tick < MAX_SONG_TICKS)
                .filter_map(|marker| marker_name(&marker.text).map(|name| (marker.tick, name)))
                .take(windfall_project::MAX_TIMELINE_ITEMS)
                .collect(),
            length,
            samples: Vec::new(),
            channels: Vec::new(),
            playlist_tracks: Vec::new(),
            patterns: Vec::new(),
            clips: Vec::new(),
            tempo_points: Vec::new(),
            adjustments: Vec::new(),
        },
        limit,
        segment,
        share,
        source_meters: if options.time_signature {
            song.time_signatures
                .iter()
                .map(|event| {
                    let signature = fit_signature(event.numerator, event.denominator);
                    (event.tick, signature.numerator, signature.denominator)
                })
                .take(windfall_project::MAX_TIMELINE_ITEMS)
                .collect()
        } else {
            Vec::new()
        },
        room: MAX_IMPORTED_NOTES,
        fitted: Fitted::default(),
        drums: Vec::new(),
        mpe_members: 0,
    };
    if options.note_expression {
        builder.mpe_members = import_expression::mpe_members(&song.tracks);
        if builder.mpe_members > 0 {
            let setup = song
                .tracks
                .iter()
                .map(import_expression::mpe_setup_messages);
            builder.fitted.controllers_kept += setup.sum::<usize>();
        }
    }
    for (index, track) in song.tracks.iter().enumerate() {
        let channels = track.note_channels();
        for &channel in &channels {
            builder.add_part(index, track, channel, channels.len() > 1);
        }
    }
    if options.tempo {
        builder.plan_tempo(&song);
    }
    builder.report(&song, first.zip(signature));
    if options.time_signature {
        let mut meters = BTreeMap::new();
        let mut dropped = 0;
        for event in &song.time_signatures {
            if event.tick >= MAX_SONG_TICKS {
                dropped += 1;
                continue;
            }
            let used = fit_signature(event.numerator, event.denominator);
            if (event.numerator, event.denominator) != (used.numerator, used.denominator) {
                builder
                    .plan
                    .adjustments
                    .push(Adjustment::TimeSignatureFitted {
                        numerator: event.numerator,
                        denominator: event.denominator,
                        used,
                    });
            }
            if meters.insert(event.tick, used).is_some() {
                dropped += 1;
            }
        }
        dropped += meters
            .len()
            .saturating_sub(windfall_project::MAX_TIMELINE_ITEMS);
        builder.plan.meters = meters
            .into_iter()
            .take(windfall_project::MAX_TIMELINE_ITEMS)
            .collect();
        if dropped > 0 {
            builder
                .plan
                .adjustments
                .push(Adjustment::TimeSignatureChanges { count: dropped });
        }
    }
    builder.plan
}

/// The nearest time signature a project can have.
fn fit_signature(numerator: u8, denominator: u8) -> TimeSignature {
    TimeSignature {
        numerator: numerator.clamp(1, 16),
        denominator: denominator.clamp(2, 16).next_power_of_two(),
    }
}

/// A name fit for a project: one line, of a length that fits a channel
/// strip. `None` when nothing is left of it.
fn clean_name(name: &str) -> Option<String> {
    let line: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cut: String = line.trim().chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_owned())
}

fn marker_name(name: &str) -> Option<String> {
    let mut result = String::new();
    for character in name.trim().chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if result.len() + character.len_utf8() > 256 {
            break;
        }
        result.push(character);
    }
    let result = result.trim().to_owned();
    (!result.is_empty()).then_some(result)
}

/// What a part is called. `shared` says that its track has notes on other
/// channels too, so that the track's name is not this part's alone.
fn part_name(track: &MidiTrack, index: usize, channel: u8, shared: bool) -> String {
    let own = |text: &Option<String>| text.as_deref().and_then(clean_name);
    let named = own(&track.name).or_else(|| own(&track.instrument));
    let program = track.program(channel).map(program_name);
    let numbered = || format!("Track {}", index + 1);
    match (channel == DRUM_CHANNEL, shared) {
        (true, false) => named.unwrap_or_else(|| "Drums".to_owned()),
        (true, true) => "Drums".to_owned(),
        (false, false) => named
            .or(program.map(str::to_owned))
            .unwrap_or_else(numbered),
        (false, true) => program.map_or_else(
            || {
                let track = named.unwrap_or_else(numbered);
                format!("{track} ch {}", channel + 1)
            },
            str::to_owned,
        ),
    }
}

/// A note of a pattern, in whole numbers so that two patterns can be told
/// apart exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Hit {
    start: u32,
    key: u8,
    length: u32,
    velocity: u8,
    release: u8,
    midi_channel: u8,
    /// The bend and controllers the note's channel has when it starts.
    values: NoteValues,
}

/// The notes one channel plays in a pattern: the channel's index in the
/// plan, and the notes in order.
type LaneHits = (usize, Vec<Hit>);

/// Phrase sharing includes its local labels and meter, as well as notes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LocalTimeline {
    signature: Option<(u8, u8)>,
    meters: Vec<(u32, u8, u8)>,
    markers: Vec<(u32, String)>,
}

/// The notes of one channel of a part, by the stretch of the song they
/// start in.
struct Lane {
    /// Index into the plan's channels.
    channel: usize,
    stretches: BTreeMap<u32, Vec<Hit>>,
}

/// What had to be fitted, counted while the plan is built.
#[derive(Default)]
struct Fitted {
    past_end: usize,
    cut: usize,
    split: usize,
    over_limit: usize,
    tempo_out_of_range: usize,
    tempo_thinned: Option<(usize, usize)>,
    /// Volume and pan messages that were put to use.
    mixed: usize,
    drum_keys_without_pad: usize,
    /// Bend, controller and pressure messages that note expression
    /// accounts for.
    bends_kept: usize,
    controllers_kept: usize,
    pressure_kept: usize,
    /// Notes that took expression from their channel, and those whose
    /// bend was more than a fine pitch holds.
    expressive: usize,
    fine_fitted: usize,
}

struct Builder<'a> {
    options: &'a ImportOptions,
    plan: ImportPlan,
    /// The last tick a note may reach.
    limit: u32,
    /// The length of a stretch of the song that gets a pattern.
    segment: u32,
    share: bool,
    source_meters: Vec<(u32, u8, u8)>,
    /// How many more notes the plan may take.
    room: usize,
    fitted: Fitted,
    /// The names of the drum parts that play a synth.
    drums: Vec<String>,
    /// How many member channels the song's lower MPE zone has, from
    /// channel 2 up. 0 when the song has no zone.
    mpe_members: u8,
}

impl Builder<'_> {
    /// Plans the channels, the playlist track, the patterns and the clips
    /// of one part: the notes `track` has on `channel`.
    fn add_part(&mut self, index: usize, track: &MidiTrack, channel: u8, shared: bool) {
        let mut kept = Vec::new();
        for note in track.notes.iter().filter(|note| note.channel == channel) {
            if note.start >= self.limit {
                self.fitted.past_end += 1;
                continue;
            }
            let end = note.end().min(self.limit);
            let pieces = ((end - 1) / self.segment - note.start / self.segment + 1) as usize;
            if pieces > self.room {
                self.fitted.over_limit += 1;
                continue;
            }
            self.room -= pieces;
            self.fitted.cut += usize::from(end < note.end());
            self.fitted.split += usize::from(pieces > 1);
            kept.push((note.start, end, note.key, note.velocity, note.release));
        }
        if kept.is_empty() {
            return;
        }
        let values = if self.options.note_expression {
            let spans: Vec<(u32, u32)> = kept.iter().map(|note| (note.0, note.1)).collect();
            let member = (1..=self.mpe_members).contains(&channel);
            let part = import_expression::read(track, channel, &spans, member);
            self.fitted.bends_kept += part.bends_kept;
            self.fitted.controllers_kept += part.controllers_kept;
            self.fitted.pressure_kept += part.pressure_kept;
            self.fitted.expressive += part.expressive;
            self.fitted.fine_fitted += part.clamped;
            part.notes
        } else {
            vec![NoteValues::NEUTRAL; kept.len()]
        };

        let options = self.options;
        let name = part_name(track, index, channel, shared);
        let drums = channel == DRUM_CHANNEL;
        let first = |controller| {
            let curve = track.control(channel, ControlKind::Controller(controller));
            let point = curve.and_then(|curve| curve.points.first());
            point
                .filter(|_| options.channel_mix)
                .map(|point| point.value)
        };
        let (volume, pan) = (first(CC_VOLUME), first(CC_PAN));
        self.fitted.mixed += usize::from(volume.is_some()) + usize::from(pan.is_some());
        let channel_for = |name: String, sound| PlannedChannel {
            name,
            sound,
            midi_track: index,
            midi_channel: channel,
            drums,
            program: track.program(channel),
            volume: volume.map(volume_from_midi),
            pan: pan.map(pan_from_midi),
            notes: 0,
        };

        // The pads the part plays, by key, each with the sample it plays.
        let kit = options.drum_kit.as_ref().filter(|_| drums);
        let keys: BTreeSet<u8> = kept.iter().map(|&(_, _, key, _, _)| key).collect();
        let mut lanes: Vec<Lane> = Vec::new();
        let mut lane_of_key: HashMap<u8, usize> = HashMap::new();
        let mut lane_of_sample: HashMap<usize, usize> = HashMap::new();
        for &key in &keys {
            let pad = kit.and_then(|kit| kit.pad(key));
            let Some(pad) = pad.filter(|pad| pad.sample.problem().is_none()) else {
                continue;
            };
            let known = &mut self.plan.samples;
            let held = known.iter().position(|sample| sample.path == pad.sample);
            let sample = held.unwrap_or_else(|| {
                known.push(PlannedSample {
                    name: pad.name.clone(),
                    path: pad.sample.clone(),
                });
                known.len() - 1
            });
            let lane = *lane_of_sample.entry(sample).or_insert_with(|| {
                let name = clean_name(&pad.name).unwrap_or_else(|| name.clone());
                let planned = channel_for(name, PlannedSound::Sample(sample));
                self.plan.channels.push(planned);
                lanes.push(Lane {
                    channel: self.plan.channels.len() - 1,
                    stretches: BTreeMap::new(),
                });
                lanes.len() - 1
            });
            lane_of_key.insert(key, lane);
        }
        // Everything else plays the synth, on the keys the file has.
        let synth = lanes.len();
        if keys.iter().any(|key| !lane_of_key.contains_key(key)) {
            self.plan
                .channels
                .push(channel_for(name.clone(), PlannedSound::Synth));
            lanes.push(Lane {
                channel: self.plan.channels.len() - 1,
                stretches: BTreeMap::new(),
            });
            if drums && kit.is_none() {
                self.drums.push(name.clone());
            }
        }

        for ((start, end, key, velocity, release), values) in kept.into_iter().zip(values) {
            let (lane, key) = match lane_of_key.get(&key) {
                Some(&lane) => (lane, DEFAULT_KEY),
                None => {
                    self.fitted.drum_keys_without_pad += usize::from(kit.is_some());
                    (synth, key)
                }
            };
            let lane = &mut lanes[lane];
            let mut at = start;
            while at < end {
                let within = at % self.segment;
                let length = (end - at).min(self.segment - within);
                let stretch = lane.stretches.entry(at / self.segment).or_default();
                stretch.push(Hit {
                    start: within,
                    key,
                    length,
                    velocity,
                    release,
                    midi_channel: channel,
                    values,
                });
                at += length;
            }
        }

        let track_index = self.plan.playlist_tracks.len();
        self.plan.playlist_tracks.push(name.clone());
        self.add_patterns(&name, track_index, lanes);
    }

    /// Plans a pattern for every stretch of the song in which one of
    /// `lanes` has notes, and a clip that places it.
    fn add_patterns(&mut self, name: &str, track: usize, mut lanes: Vec<Lane>) {
        let stretches: BTreeSet<u32> = lanes
            .iter()
            .flat_map(|lane| lane.stretches.keys().copied())
            .collect();
        let first_pattern = self.plan.patterns.len();
        // The patterns made so far, by their length and their notes.
        let mut shared: HashMap<(u32, Vec<LaneHits>, LocalTimeline), usize> = HashMap::new();
        for stretch in stretches {
            let start = stretch * self.segment;
            let length = self.segment.min(self.plan.length - start);
            let length_steps = length / TICKS_PER_STEP;
            let timeline = LocalTimeline {
                signature: self.options.time_signature.then(|| {
                    self.source_meters
                        .iter()
                        .rev()
                        .find(|(tick, _, _)| *tick <= start)
                        .map(|(_, numerator, denominator)| (*numerator, *denominator))
                        .unwrap_or((4, 4))
                }),
                meters: self
                    .source_meters
                    .iter()
                    .filter(|(tick, _, _)| *tick > start && *tick < start + length)
                    .map(|(tick, numerator, denominator)| (*tick - start, *numerator, *denominator))
                    .collect(),
                markers: self
                    .plan
                    .markers
                    .iter()
                    .filter(|(tick, _)| *tick >= start && *tick < start + length)
                    .map(|(tick, name)| (*tick - start, name.clone()))
                    .collect(),
            };
            let mut content = Vec::new();
            for lane in &mut lanes {
                if let Some(mut hits) = lane.stretches.remove(&stretch) {
                    hits.sort_unstable();
                    content.push((lane.channel, hits));
                }
            }
            let new_pattern = |plan: &mut ImportPlan, content: &[LaneHits]| {
                for (channel, hits) in content {
                    plan.channels[*channel].notes += hits.len();
                }
                plan.patterns.push(PlannedPattern {
                    name: String::new(),
                    length_steps,
                    time_signature: timeline.signature.map(|(numerator, denominator)| {
                        TimeSignature {
                            numerator,
                            denominator,
                        }
                    }),
                    meters: timeline
                        .meters
                        .iter()
                        .map(|&(tick, numerator, denominator)| {
                            (
                                tick,
                                TimeSignature {
                                    numerator,
                                    denominator,
                                },
                            )
                        })
                        .collect(),
                    markers: timeline.markers.clone(),
                    lanes: content.iter().map(planned_lane).collect(),
                });
                plan.patterns.len() - 1
            };
            let pattern = if self.share {
                match shared.entry((length_steps, content, timeline.clone())) {
                    Entry::Occupied(known) => *known.get(),
                    Entry::Vacant(unknown) => {
                        let pattern = new_pattern(&mut self.plan, &unknown.key().1);
                        *unknown.insert(pattern)
                    }
                }
            } else {
                new_pattern(&mut self.plan, &content)
            };
            self.plan.clips.push(PlannedClip {
                pattern,
                track,
                start,
                length,
            });
        }

        // A part with one pattern gives it its name; with several they
        // are numbered.
        let patterns = &mut self.plan.patterns[first_pattern..];
        let several = patterns.len() > 1;
        for (number, pattern) in patterns.iter_mut().enumerate() {
            pattern.name = if several {
                format!("{name} {}", number + 1)
            } else {
                name.to_owned()
            };
        }
    }

    /// Plans the project's tempo and, if the tempo changes, the points of
    /// its automation.
    fn plan_tempo(&mut self, song: &MidiSong) {
        if song.tempos.is_empty() {
            return;
        }
        let mut fit = |micros| {
            let tempo = bpm(micros);
            let fitted = tempo.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM);
            self.fitted.tempo_out_of_range += usize::from(fitted != tempo);
            fitted
        };
        // The tempo on each tick it changes on, from the start of the
        // song. Of several events on one tick the last counts.
        let mut steps = vec![(0, bpm(DEFAULT_MICROS_PER_QUARTER))];
        let mut first = None;
        for tempo in &song.tempos {
            let fitted = fit(tempo.micros_per_quarter);
            // A tempo set where the song ends has nothing left to play at.
            if tempo.tick >= self.plan.length {
                continue;
            }
            first.get_or_insert(fitted);
            match steps.last_mut() {
                Some(last) if last.0 == tempo.tick => last.1 = fitted,
                _ => steps.push((tempo.tick, fitted)),
            }
        }
        self.plan.tempo_bpm = first;
        steps.dedup_by(|next, kept| next.1 == kept.1);
        if steps.len() < 2 {
            return;
        }

        let changes = steps.len();
        if changes > MAX_AUTOMATION_POINTS {
            // Evenly spread, with the first and the last among them.
            let last = (changes - 1) as u64;
            let most = (MAX_AUTOMATION_POINTS - 1) as u64;
            steps = (0..=most)
                .map(|index| steps[(index * last / most) as usize])
                .collect();
            self.fitted.tempo_thinned = Some((changes, steps.len()));
        }
        self.plan.tempo_points = steps
            .into_iter()
            .map(|(tick, tempo)| AutomationPoint {
                tick,
                value: AutomationRange::TEMPO.normalized(tempo as f32),
                curve: 0.0,
                hold: true,
            })
            .collect();
    }

    /// Lists what was fitted and what the song holds that was not
    /// imported.
    fn report(
        &mut self,
        song: &MidiSong,
        signature: Option<(&crate::song::TimeSignatureChange, TimeSignature)>,
    ) {
        let fitted = &self.fitted;
        let mut list = Vec::new();
        let mut count = |count: usize, adjustment: fn(usize) -> Adjustment| {
            if count > 0 {
                list.push(adjustment(count));
            }
        };
        count(fitted.past_end, |count| Adjustment::NotesPastEnd { count });
        count(fitted.cut, |count| Adjustment::NotesCutAtEnd { count });
        count(fitted.split, |count| Adjustment::NotesSplit { count });
        count(fitted.over_limit, |count| Adjustment::NotesOverLimit {
            count,
        });
        count(fitted.tempo_out_of_range, |count| {
            Adjustment::TempoOutOfRange { count }
        });
        count(fitted.drum_keys_without_pad, |notes| {
            Adjustment::DrumKeysWithoutPad { notes }
        });

        let mut points = [0; 3];
        let mut programs = 0;
        for track in &song.tracks {
            programs += track.programs.len();
            for curve in &track.controls {
                let kind = match curve.kind {
                    ControlKind::PitchBend => 0,
                    ControlKind::Controller(_) => 1,
                    ControlKind::ChannelPressure | ControlKind::KeyPressure(_) => 2,
                };
                points[kind] += curve.points.len();
            }
        }
        let [bends, controllers, pressure] = points;
        let mut left = |what, count: usize| {
            if count > 0 {
                list.push(Adjustment::NotImported { what, count });
            }
        };
        left(
            Unsupported::PitchBend,
            bends.saturating_sub(fitted.bends_kept),
        );
        left(
            Unsupported::Controllers,
            (controllers - fitted.mixed).saturating_sub(fitted.controllers_kept),
        );
        left(
            Unsupported::Aftertouch,
            pressure.saturating_sub(fitted.pressure_kept),
        );
        left(Unsupported::ProgramChanges, programs);
        left(Unsupported::KeySignatures, song.key_signatures.len());
        left(
            Unsupported::Markers,
            song.markers.len().saturating_sub(self.plan.markers.len()),
        );

        if let Some((changes, kept)) = fitted.tempo_thinned {
            list.push(Adjustment::TempoChangesThinned { changes, kept });
        }
        if fitted.expressive > 0 {
            list.push(Adjustment::ExpressionImported {
                notes: fitted.expressive,
            });
        }
        if fitted.fine_fitted > 0 {
            list.push(Adjustment::FinePitchFitted {
                notes: fitted.fine_fitted,
            });
        }
        let _ = signature; // Fitted meter events are reported individually above.
        let drums = self.drums.drain(..);
        list.extend(drums.map(|channel| Adjustment::DrumsAsSynth { channel }));
        self.plan.adjustments = list;
    }
}

fn planned_lane((channel, hits): &LaneHits) -> PlannedLane {
    PlannedLane {
        channel: *channel,
        notes: hits
            .iter()
            .map(|hit| NoteInit {
                start: hit.start,
                length: hit.length,
                key: hit.key,
                velocity: Some(velocity_from_midi(hit.velocity)),
                pan: None,
                expression: Some(windfall_project::NoteExpression {
                    release: f32::from(hit.release.min(127)) / 128.0,
                    color_group: Some(hit.midi_channel),
                    fine_pitch_cents: hit.values.fine_pitch_cents().0,
                    modulation_x: timbre_from_midi(hit.values.x),
                    modulation_y: pressure_from_midi(hit.values.y),
                    ..Default::default()
                }),
            })
            .collect(),
    }
}

impl ImportPlan {
    /// True when the import would add nothing to a project: the song has
    /// no notes and no tempo changes.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty() && self.tempo_points.is_empty() && self.meters.is_empty()
    }

    /// How many notes the import adds to the project's patterns.
    pub fn note_count(&self) -> usize {
        self.channels.iter().map(|channel| channel.notes).sum()
    }

    /// The import as one command for `project`: a [`Command::Batch`]
    /// labelled "Import MIDI", which is one undo step.
    ///
    /// The commands of a batch cannot hand ids to one another, so the ids
    /// the new channels, patterns and tracks will get are worked out here,
    /// from the project's `next_id`. The command is therefore for this
    /// project as it is now: dispatch it before anything else changes the
    /// project, or build it again.
    pub fn command(&self, project: &Project) -> Command {
        Command::Batch {
            label: Some(LABEL.to_owned()),
            commands: self.commands(project),
        }
    }

    /// The commands [`command`](Self::command) wraps, in order: the
    /// project's settings, the samples, the channels with their volumes
    /// and pans, the playlist tracks, each pattern with its length and
    /// notes, the clips, and last the tempo automation with its playlist
    /// track and clip.
    pub fn commands(&self, project: &Project) -> Vec<Command> {
        let mut ids = Ids {
            next: project.next_id,
            mixer_tracks: project
                .mixer
                .tracks
                .iter()
                .filter(|track| !track.current)
                .count(),
        };
        let mut commands = Vec::new();
        if self.tempo_bpm.is_some() || self.time_signature.is_some() {
            commands.push(Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: self.tempo_bpm,
                    time_signature: self.time_signature,
                    ..SettingsPatch::default()
                },
            });
        }

        let mut samples = Vec::with_capacity(self.samples.len());
        for sample in &self.samples {
            // A sample the project already has keeps its id, and adding
            // it again hands out none.
            let held = project.samples.iter().find(|held| held.path == sample.path);
            samples.push(held.map_or_else(|| SampleId(ids.take()), |held| held.id));
            commands.push(Command::AddSample {
                name: sample.name.clone(),
                path: sample.path.clone(),
            });
        }

        let mut channels = Vec::with_capacity(self.channels.len());
        for channel in &self.channels {
            let id = ChannelId(ids.take());
            ids.take_mixer_track();
            channels.push(id);
            let (sample, instrument) = match channel.sound {
                PlannedSound::Synth => (None, Some(InstrumentKind::SubtractiveSynth)),
                PlannedSound::Sample(index) => (samples.get(index).copied(), None),
            };
            commands.push(Command::AddChannel {
                name: Some(channel.name.clone()),
                sample,
                instrument,
                index: None,
                mixer_track: None,
            });
            if channel.volume.is_some() || channel.pan.is_some() {
                commands.push(Command::UpdateChannel {
                    id,
                    patch: ChannelPatch {
                        volume: channel.volume,
                        pan: channel.pan,
                        ..ChannelPatch::default()
                    },
                });
            }
        }

        let mut tracks = Vec::with_capacity(self.playlist_tracks.len());
        for name in &self.playlist_tracks {
            tracks.push(PlaylistTrackId(ids.take()));
            commands.push(Command::AddPlaylistTrack {
                name: Some(name.clone()),
                index: None,
            });
        }

        let mut patterns = Vec::with_capacity(self.patterns.len());
        for pattern in &self.patterns {
            let id = PatternId(ids.take());
            patterns.push(id);
            commands.push(Command::AddPattern {
                name: Some(pattern.name.clone()),
            });
            commands.push(Command::UpdatePattern {
                id,
                patch: PatternPatch {
                    length_steps: Some(pattern.length_steps),
                    ..PatternPatch::default()
                },
            });
            let mut timeline = Timeline::default();
            if let Some(signature) = pattern.time_signature {
                commands.push(Command::EditPatternTimeline {
                    pattern: id,
                    expected: timeline.clone(),
                    expected_signature: None,
                    edit: PatternTimelineEdit::SetSignature {
                        signature: Some(signature),
                    },
                });
            }
            for &(tick, signature) in &pattern.meters {
                commands.push(Command::EditPatternTimeline {
                    pattern: id,
                    expected: timeline.clone(),
                    expected_signature: pattern.time_signature,
                    edit: PatternTimelineEdit::AddMeter { tick, signature },
                });
                timeline.meters.push(MeterChange {
                    id: MeterChangeId(ids.take()),
                    tick,
                    signature,
                });
            }
            for (tick, name) in &pattern.markers {
                commands.push(Command::EditPatternTimeline {
                    pattern: id,
                    expected: timeline.clone(),
                    expected_signature: pattern.time_signature,
                    edit: PatternTimelineEdit::AddMarker {
                        tick: *tick,
                        name: name.clone(),
                    },
                });
                timeline.markers.push(TimelineMarker {
                    id: TimelineMarkerId(ids.take()),
                    tick: *tick,
                    name: name.clone(),
                    kind: MarkerKind::Named,
                });
            }
            for lane in &pattern.lanes {
                let Some(&channel) = channels.get(lane.channel) else {
                    continue;
                };
                ids.skip(lane.notes.len());
                commands.push(Command::AddNotes {
                    pattern: id,
                    channel,
                    notes: lane.notes.clone(),
                });
            }
        }

        let clips: Vec<ClipInit> = self
            .clips
            .iter()
            .filter_map(|clip| {
                let pattern = *patterns.get(clip.pattern)?;
                Some(ClipInit {
                    track: *tracks.get(clip.track)?,
                    start: clip.start,
                    length: Some(clip.length),
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern { pattern },
                })
            })
            .collect();
        if !clips.is_empty() {
            ids.skip(clips.len());
            commands.push(Command::AddClips { clips });
        }

        if !self.tempo_points.is_empty() {
            let automation = AutomationId(ids.take());
            commands.push(Command::AddAutomation {
                name: None,
                target: AutomationTarget::Tempo,
                points: Some(self.tempo_points.clone()),
            });
            let track = PlaylistTrackId(ids.take());
            commands.push(Command::AddPlaylistTrack {
                name: Some("Tempo".to_owned()),
                index: None,
            });
            commands.push(Command::AddClips {
                clips: vec![ClipInit {
                    track,
                    start: 0,
                    length: Some(self.length),
                    offset: None,
                    muted: None,
                    content: ClipContent::Automation { automation },
                }],
            });
        }
        if !self.meters.is_empty() {
            commands.extend(
                project
                    .playlist
                    .timeline
                    .meters
                    .iter()
                    .map(|m| Command::RemoveMeterChange { id: m.id }),
            );
            commands.extend(
                self.meters
                    .iter()
                    .map(|&(tick, signature)| Command::AddMeterChange { tick, signature }),
            );
        }
        commands.extend(
            self.markers
                .iter()
                .map(|(tick, name)| Command::AddTimelineMarker {
                    tick: *tick,
                    name: name.clone(),
                    kind: MarkerKind::Named,
                }),
        );
        commands
    }
}

/// Follows the ids a project hands out as the commands of a batch run.
struct Ids {
    next: u32,
    mixer_tracks: usize,
}

impl Ids {
    fn take(&mut self) -> u32 {
        let id = self.next;
        self.skip(1);
        id
    }

    /// Passes over ids that the notes or clips of one command take.
    fn skip(&mut self, count: usize) {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        self.next = self.next.saturating_add(count);
    }

    /// A new channel gets a mixer track of its own, which takes the id
    /// after the channel's, for as long as the mixer has room.
    fn take_mixer_track(&mut self) {
        if self.mixer_tracks < MAX_MIXER_SIGNAL_TRACKS {
            self.mixer_tracks += 1;
            self.skip(1);
        }
    }
}
