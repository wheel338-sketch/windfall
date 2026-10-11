//! Every edit to a project is a [`Command`].
//!
//! The UI, the command palette, scripting and the assistant all change a
//! project the same way: they build a command and dispatch it to the
//! [`Document`](crate::Document). Commands are plain data, so they cross the
//! IPC boundary as JSON.
//!
//! Patch structs hold one `Option` per field. `None` leaves the field alone.
//!
//! A number outside the range its field documents is brought into the range,
//! so a control can send what it has. Input that has no nearest valid value
//! fails the command instead: an id that does not exist, a key above 127, a
//! note or clip with no length, a color above 0xFFFFFF, NaN. An index past
//! the end of a list means the end.
//!
//! Time has two hard ends. A note cannot be added at, or moved to, a tick
//! at or past [`MAX_PATTERN_TICKS`](crate::model::MAX_PATTERN_TICKS), the
//! end of the longest pattern, where it could never play. A clip cannot end
//! past [`MAX_SONG_TICKS`](crate::model::MAX_SONG_TICKS), the end of the
//! longest song, and its offset cannot be larger than that either. Both
//! fail the command with a message that says so; a UI keeps its own edits
//! inside them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{
    AutomationId, AutomationPoint, AutomationTarget, ChannelId, ClipContent, ClipId, EffectId,
    EffectKind, EffectParams, Envelope, InstrumentKind, InstrumentParams, NoteId, PatternId,
    PlaylistTrackId, SampleId, SamplePath, TimeSignature, TrackId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
// Keep parameter payloads inline in the public command API; boxing adds an
// allocation to parameter edits and changes Rust callers' construction contract.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Creates an arrangement ID from the document allocator.
    AddArrangement {
        name: String,
        clips: Vec<ClipId>,
        tracks: Vec<PlaylistTrackId>,
    },
    RenameArrangement {
        id: crate::arrangement::ArrangementId,
        name: String,
    },
    SetArrangementReferences {
        id: crate::arrangement::ArrangementId,
        clips: Vec<ClipId>,
        tracks: Vec<PlaylistTrackId>,
    },
    /// Changes the active metadata ID; playback still uses the single playlist.
    SwitchArrangement {
        id: crate::arrangement::ArrangementId,
    },
    RemoveArrangement {
        id: crate::arrangement::ArrangementId,
    },
    AddTrackGroup {
        name: String,
        parent: Option<crate::arrangement::TrackGroupId>,
    },
    RenameTrackGroup {
        id: crate::arrangement::TrackGroupId,
        name: String,
    },
    MoveTrackGroup {
        id: crate::arrangement::TrackGroupId,
        parent: Option<crate::arrangement::TrackGroupId>,
    },
    MoveTrackToGroup {
        track: PlaylistTrackId,
        parent: Option<crate::arrangement::TrackGroupId>,
    },
    RemoveTrackGroup {
        id: crate::arrangement::TrackGroupId,
    },
    AddClipGroup {
        clips: Vec<ClipId>,
    },
    RemoveClipGroup {
        id: crate::arrangement::ClipGroupId,
    },
    LinkTrack {
        track: PlaylistTrackId,
        kind: Option<crate::arrangement::TrackKind>,
    },
    /// Copies the chosen clip's source and redirects only that clip. Creates the source ID.
    MakeUnique {
        clip: ClipId,
    },
    /// Adds an alternative timeline layout and makes it the active one.
    /// `copy` clones every clip of the layout that is active now as a new
    /// clip, so later moves, resizes and deletions in either layout leave
    /// the other alone; without it the new layout starts with the same
    /// tracks and no clips. A project that has no arrangement yet first
    /// keeps its present timeline as "Arrangement 1". Creates the
    /// arrangement id and then one [`ClipId`] per copied clip.
    AddArrangementLayout {
        name: String,
        copy: bool,
    },
    /// Adds a channel with its mixer track and a playlist track linked to
    /// it, as one undo step. Pattern clips on the track play only that
    /// channel. Creates a [`ChannelId`], then a [`TrackId`] when the mixer
    /// has room, then a [`PlaylistTrackId`].
    AddInstrumentTrack {
        #[serde(default)]
        #[ts(optional)]
        name: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        sample: Option<SampleId>,
        #[serde(default)]
        #[ts(optional)]
        instrument: Option<InstrumentKind>,
    },
    /// Adds a mixer track and a playlist track linked to it, as one undo
    /// step. Audio clips on the track play into that mixer track. Creates a
    /// [`TrackId`] and then a [`PlaylistTrackId`].
    AddAudioTrack {
        #[serde(default)]
        #[ts(optional)]
        name: Option<String>,
    },
    AddNotesWithCurves {
        pattern: PatternId,
        channel: ChannelId,
        notes: Vec<NoteInit>,
        curves: Vec<crate::NoteCurveInsert>,
    },
    SetNoteExpressionCurves {
        pattern: PatternId,
        channel: ChannelId,
        expected: Vec<crate::Note>,
        expected_curves: Vec<crate::NoteExpressionCurve>,
        curves: Vec<crate::NoteExpressionCurve>,
    },
    /// Replaces the event automation a pattern stores for `target`: a curve
    /// in pattern ticks that moves the control wherever the pattern plays.
    /// `expected` is the curve as the editor captured it, empty when the
    /// pattern had none; the command fails when it no longer matches. No
    /// points removes the lane. Values are brought into 0 to 1 and curves
    /// into -1 to 1. Fails for the tempo, for a target the project does not
    /// have, for points out of order or past the longest pattern, and past
    /// 64 lanes in one pattern. A new lane creates an
    /// [`AutomationId`]. Dispatched with a gesture id, a drag is one undo
    /// step.
    SetPatternEvents {
        pattern: PatternId,
        target: AutomationTarget,
        expected: Vec<AutomationPoint>,
        points: Vec<AutomationPoint>,
    },
    /// Writes a bounded musical LFO into a range of a pattern's event lane
    /// in one edit, creating the lane at the target's stored value when the
    /// pattern has none. `expected` is checked as for
    /// [`Command::SetPatternEvents`].
    GeneratePatternEventLfo {
        pattern: PatternId,
        target: AutomationTarget,
        expected: Vec<AutomationPoint>,
        start: u32,
        end: u32,
        resolution: u32,
        lfo: crate::CurveLfo,
    },
    /// Writes a musical LFO into the expression curves of captured notes:
    /// one continuous curve per note for `parameter`, sampled every
    /// `resolution` ticks along the note from `origin`, and blended with the
    /// value the note had there by `strength`. The captured notes and their
    /// curves are checked as for [`Command::SetNoteExpressionCurves`].
    GenerateNoteCurveLfo {
        pattern: PatternId,
        channel: ChannelId,
        expected: Vec<crate::Note>,
        expected_curves: Vec<crate::NoteExpressionCurve>,
        parameter: crate::NoteCurveParameter,
        origin: u32,
        resolution: u32,
        strength: f64,
        lfo: crate::CurveLfo,
    },
    SetPluginSidechainInput {
        target: crate::PluginTarget,
        input: Option<u32>,
    },
    SetSidechain {
        from: TrackId,
        to: TrackId,
        #[ts(optional)]
        gain: Option<f32>,
    },
    /// Sends one separately routable output of an effect to another mixer
    /// track, or disconnects it when `target` is absent. Refused when the
    /// effect has no such output, sits on the master, or the route would
    /// make routing loop back on itself.
    SetEffectOutput {
        effect: EffectId,
        output: u8,
        #[ts(optional)]
        target: Option<TrackId>,
    },
    ApplyMixerTrackPreset {
        id: TrackId,
        expected: crate::MixerTrack,
        preset: crate::MixerTrackPreset,
        name_color: bool,
    },
    MoveMixerTracks {
        expected: Vec<TrackId>,
        ids: Vec<TrackId>,
        before: Option<TrackId>,
    },
    SetTrackParam {
        id: TrackId,
        param: u32,
        value: f32,
    },
    /// Creates the saved follow-selection utility, or selects its existing id.
    EnsureCurrentMixerTrack,
    SetTrackExternalOutput {
        id: TrackId,
        route: Option<crate::ExternalOutputRoute>,
    },
    CreateAudioTakeGroup {
        name: String,
        lanes: Vec<crate::AudioTakeLane>,
    },
    RenameAudioTakeGroup {
        id: u32,
        name: String,
    },
    RemoveAudioTakeGroup {
        id: u32,
    },
    AuditionAudioTakeGroup {
        id: u32,
        pass: Option<u16>,
    },
    CompAudioTakeGroup {
        expected: crate::AudioTakeGroup,
        sources: Vec<crate::Clip>,
        ranges: Vec<crate::TakeCompRange>,
        name: String,
        fade_ticks: u32,
        mute_sources: bool,
        replace_comp: bool,
    },
    /// Build editable audio ranges from captured retained takes as one undo step.
    CompAudioClips {
        sources: Vec<crate::Clip>,
        segments: Vec<crate::AudioCompSegment>,
        destination: Option<PlaylistTrackId>,
        name: String,
        fade_ticks: u32,
        mute_sources: bool,
    },
    /// Adds a meter change without moving any notes, clips or automation.
    AddMeterChange {
        tick: u32,
        signature: TimeSignature,
    },
    UpdateMeterChange {
        id: crate::MeterChangeId,
        tick: u32,
        signature: TimeSignature,
    },
    RemoveMeterChange {
        id: crate::MeterChangeId,
    },
    AddTimelineMarker {
        tick: u32,
        name: String,
        kind: crate::MarkerKind,
    },
    UpdateTimelineMarker {
        marker: crate::TimelineMarker,
    },
    RemoveTimelineMarker {
        id: crate::TimelineMarkerId,
    },
    /// Adds an instrument channel and binds its hosted plugin in one undo step.
    AddPluginInstrument {
        plugin: crate::PluginBinding,
    },
    /// Adds a hosted effect to the track in one undo step.
    AddPluginEffect {
        track: TrackId,
        plugin: crate::PluginBinding,
    },
    /// Moves one discovered parameter in native units, by stable id.
    SetPluginParam {
        target: crate::PluginTarget,
        id: u32,
        value: f32,
    },
    /// Stores a complete host snapshot, preserving instance identity.
    SetPluginState {
        target: crate::PluginTarget,
        state: Vec<u8>,
    },
    // Project
    /// Captured pattern-local signature, meter changes and named labels.
    EditPatternTimeline {
        pattern: PatternId,
        expected: crate::Timeline,
        expected_signature: Option<TimeSignature>,
        edit: crate::PatternTimelineEdit,
    },
    UpdateSettings {
        patch: SettingsPatch,
    },
    /// Replaces all saved notebook pages in one undo step. Trims titles only.
    ReplaceNotebook {
        notebook: crate::Notebook,
    },
    /// Replaces a channel's sound and settings with a channel preset's, as
    /// one undo step. The channel keeps its id, group, mute, solo and mixer
    /// track. `expected` is the channel as it was when the preset was
    /// chosen: a channel that changed since is refused. A sampler preset's
    /// audio is `sample`, a sample the project has, or `audio`, a file that
    /// is added to the pool in the same step; giving both fails. With
    /// `audio` the command creates a [`SampleId`], or reports the one the
    /// project already has for that file. Automation of instrument
    /// settings the preset replaces is removed. [`crate::preset`] has the
    /// rules.
    ApplyChannelPreset {
        id: ChannelId,
        expected: crate::Channel,
        preset: crate::ChannelPreset,
        #[serde(default)]
        #[ts(optional)]
        sample: Option<SampleId>,
        #[serde(default)]
        #[ts(optional)]
        audio: Option<crate::PresetAudio>,
        name_color: bool,
    },
    /// Replaces the state of one instrument or effect with a plugin
    /// preset's, as one undo step. `expected` is the state the target had
    /// when the preset was chosen. Automation of settings the preset
    /// replaces with another processor's is removed.
    ApplyPluginPreset {
        target: crate::PluginTarget,
        expected: crate::PluginPresetState,
        preset: crate::PluginPreset,
    },

    // Sample pool
    /// Registers an audio file. Creates a [`SampleId`]. If a sample with the
    /// same path is already in the pool, nothing is added and its id is
    /// reported as the created id.
    AddSample {
        name: String,
        path: SamplePath,
    },
    /// Fails while any channel or audio clip still uses the sample.
    RemoveSample {
        id: SampleId,
    },

    // Channel rack
    /// Adds a channel and, unless `mixer_track` is given, a new mixer track
    /// routed to the master that the channel plays into. Creates a
    /// [`ChannelId`] and then, if a track was made, a [`TrackId`]. When the
    /// mixer is full, no track is made and the channel plays into the master.
    ///
    /// The channel is a sampler unless `instrument` is given. Giving both
    /// `sample` and `instrument` fails.
    AddChannel {
        #[serde(default)]
        #[ts(optional)]
        /// Defaults to the sample name, the instrument's name, or "Sampler".
        name: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        sample: Option<SampleId>,
        #[serde(default)]
        #[ts(optional)]
        /// Makes the channel play this instrument, at its default settings.
        instrument: Option<InstrumentKind>,
        #[serde(default)]
        #[ts(optional)]
        /// Position in the rack. Defaults to the end.
        index: Option<u32>,
        #[serde(default)]
        #[ts(optional)]
        mixer_track: Option<TrackId>,
    },
    /// Removes the channel and its notes from every pattern, and every
    /// automation of the channel with its clips. The mixer track it was
    /// routed to stays.
    RemoveChannel {
        id: ChannelId,
    },
    /// Copies the channel, with its sampler or instrument settings, and its
    /// notes in every pattern. The copy goes right below the original and
    /// shares its mixer track. Its name is the original's with a number, as
    /// in "Kick #2". Creates a [`ChannelId`].
    DuplicateChannel {
        id: ChannelId,
    },
    MoveChannel {
        id: ChannelId,
        index: u32,
    },
    /// A new name is given to the channel's mixer track as well, in the
    /// same undo step, when that track is the channel's own and still has
    /// the channel's old name, as the track made with a channel does. The
    /// track is the channel's own when nothing else plays into it: no
    /// other channel, no audio clip and no other track. A track that was
    /// given a name of its own keeps it, and so does the track a channel
    /// leaves in the same command.
    /// Replaces all channel voice tools in one undoable edit.
    SetChannelVoiceSettings {
        id: ChannelId,
        settings: crate::ChannelVoiceSettings,
    },
    UpdateChannel {
        id: ChannelId,
        patch: ChannelPatch,
    },
    /// Replaces what the channel's per-note modulation X, modulation Y and
    /// release drive, in one undoable edit. A map with an amount, pitch
    /// range or bend range outside its range is refused.
    SetChannelExpressionMap {
        id: ChannelId,
        map: crate::ExpressionMap,
    },
    /// Moves every listed channel to a named group in one undo entry.
    /// An empty group name means ungrouped. All channel ids are checked
    /// before any membership changes; repeated ids are ignored.
    SetChannelGroup {
        channels: Vec<ChannelId>,
        group: String,
    },
    /// Renames a nonempty existing group; an existing target name merges
    /// the groups without changing rack order or musical data.
    RenameChannelGroup {
        name: String,
        new_name: String,
    },
    /// Clears this group's memberships. Channels and their music stay.
    RemoveChannelGroup {
        name: String,
    },
    /// Fails on an instrument channel.
    SetChannelSample {
        id: ChannelId,
        #[serde(default)]
        #[ts(optional)]
        sample: Option<SampleId>,
    },
    /// Fails on an instrument channel.
    UpdateSampler {
        id: ChannelId,
        patch: SamplerPatch,
    },
    /// Turns the sampler's volume envelope on (`Some`) or off (`None`).
    /// Fails on an instrument channel.
    SetSamplerEnvelope {
        id: ChannelId,
        #[serde(default)]
        #[ts(optional)]
        envelope: Option<Envelope>,
    },
    /// Sets one setting of the channel's instrument. `param` is its index
    /// in the descriptors of the instrument's kind, and the value is given
    /// the way the descriptors describe it: a toggle as 0 or 1, a choice as
    /// the index of the choice. Fails on a sampler channel, and when the
    /// instrument has no setting with that index. The history label names
    /// the setting, as in "Change Cutoff".
    SetInstrumentParam {
        channel: ChannelId,
        param: u32,
        value: f32,
    },
    /// Replaces every setting of the channel's instrument, as when a preset
    /// is loaded. Fails on a sampler channel, and when the settings are for
    /// another kind of instrument than the channel has.
    SetInstrumentParams {
        channel: ChannelId,
        params: InstrumentParams,
    },

    // Patterns
    /// Creates a [`PatternId`].
    AddPattern {
        #[serde(default)]
        #[ts(optional)]
        /// Defaults to "Pattern N".
        name: Option<String>,
    },
    /// Removes the pattern and every playlist clip that plays it. Fails on
    /// the last remaining pattern.
    RemovePattern {
        id: PatternId,
    },
    /// Creates a [`PatternId`]. The copy goes right after the original. Its
    /// name is the original's with a number, as in "Pattern 1 #2".
    DuplicatePattern {
        id: PatternId,
    },
    MovePattern {
        id: PatternId,
        index: u32,
    },
    UpdatePattern {
        id: PatternId,
        patch: PatternPatch,
    },

    // Notes
    /// Step sequencer click. If the channel has a note starting exactly at
    /// this step, removes every note that starts there. Otherwise adds a
    /// one-step note at [`DEFAULT_KEY`](crate::model::DEFAULT_KEY) and
    /// creates a [`NoteId`].
    ToggleStep {
        pattern: PatternId,
        channel: ChannelId,
        step: u32,
    },
    /// Creates one [`NoteId`] per note, in order.
    AddNotes {
        pattern: PatternId,
        channel: ChannelId,
        notes: Vec<NoteInit>,
    },
    /// Applies a reviewed step fill to [start_step, end_step). The complete
    /// lane and pattern length must still match the preview. Replacement
    /// changes only notes whose starts are in the range; overlay keeps all
    /// existing notes and skips occupied onsets. Notes outside the range,
    /// including hidden notes past the pattern end, retain their identities.
    /// Output must have unique grid-aligned starts and one-step-or-shorter
    /// lengths. Exact matching replacement notes retain their identities.
    /// Everything forms one undo entry; an identical fill is a no-op.
    FillStepRange {
        pattern: PatternId,
        channel: ChannelId,
        length_steps: u32,
        expected: Vec<crate::Note>,
        start_step: u32,
        end_step: u32,
        replace: bool,
        notes: Vec<NoteInit>,
    },
    RemoveNotes {
        pattern: PatternId,
        channel: ChannelId,
        notes: Vec<NoteId>,
    },
    /// The history label follows what the updates do: "Move note" when a
    /// start or a key changes, and "Resize note" when only lengths do,
    /// which includes a note dragged by its start whose end stays put.
    UpdateNotes {
        pattern: PatternId,
        channel: ChannelId,
        updates: Vec<NoteUpdate>,
    },
    /// An absolute property edit against captured note identities and values.
    /// Refuses the whole edit if any captured note changed or disappeared.
    UpdateCapturedNotes {
        pattern: PatternId,
        channel: ChannelId,
        expected: Vec<crate::Note>,
        updates: Vec<NoteUpdate>,
    },
    /// Removes every note the channel has in the pattern.
    ClearLane {
        pattern: PatternId,
        channel: ChannelId,
    },
    /// Transform only these captured selected notes. Every id must still be
    /// in this pattern/channel with exactly these properties, otherwise the
    /// whole command fails. Empty selections fail. Pattern extension and all
    /// changed/added/removed notes form one undo entry; no-ops add no history.
    TransformNotes {
        pattern: PatternId,
        channel: ChannelId,
        notes: Vec<crate::Note>,
        transform: crate::NoteTransform,
    },

    // Mixer
    /// Adds a track routed to the master. Creates a [`TrackId`]. Fails when
    /// the mixer already holds
    /// [`MAX_MIXER_TRACKS`](crate::model::MAX_MIXER_TRACKS).
    AddMixerTrack {
        #[serde(default)]
        #[ts(optional)]
        /// Defaults to "Insert N".
        name: Option<String>,
    },
    /// Fails on the master. Channels, audio clips and tracks that were
    /// routed to the removed track are rerouted to the master, and sends to
    /// it are dropped. The track's effects go with it, and so does every
    /// automation of the track, of its effects, and of the sends from it
    /// and to it, with their clips.
    RemoveMixerTrack {
        id: TrackId,
    },
    UpdateMixerTrack {
        id: TrackId,
        patch: MixerTrackPatch,
    },
    /// Fails on the master, and when the change would create a routing cycle.
    SetTrackOutput {
        id: TrackId,
        #[serde(default)]
        #[ts(optional)]
        output: Option<TrackId>,
    },
    /// Adds or changes a send (`Some`), or removes it (`None`). Fails when
    /// the send would create a routing cycle, and on a send from the master.
    /// Removing a send removes the automations of its level with their
    /// clips.
    SetSend {
        from: TrackId,
        to: TrackId,
        #[serde(default)]
        #[ts(optional)]
        gain: Option<f32>,
    },

    // Effects
    /// Puts a new effect of this kind on a mixer track, switched on, at
    /// full mix and with its default settings. Creates an [`EffectId`].
    /// Fails when the track already holds
    /// [`MAX_EFFECT_SLOTS`](crate::model::MAX_EFFECT_SLOTS).
    AddEffect {
        track: TrackId,
        kind: EffectKind,
        #[serde(default)]
        #[ts(optional)]
        /// Position in the chain. Defaults to the end.
        index: Option<u32>,
    },
    /// Removes the effect, and every automation of it with its clips.
    RemoveEffect {
        track: TrackId,
        effect: EffectId,
    },
    /// Moves an effect to `index` of its own track's chain or, with
    /// `to_track`, of another track's. The effect keeps its id and its
    /// settings, and its automations follow it to the other track. Fails
    /// when the other track is full.
    MoveEffect {
        track: TrackId,
        effect: EffectId,
        #[serde(default)]
        #[ts(optional)]
        to_track: Option<TrackId>,
        index: u32,
    },
    UpdateEffect {
        track: TrackId,
        effect: EffectId,
        patch: EffectSlotPatch,
    },
    /// Sets one setting of an effect. `param` is its index in the
    /// descriptors of the effect's kind, and the value is given the way the
    /// descriptors describe it: a toggle as 0 or 1, a choice as the index
    /// of the choice. Fails when the effect has no setting with that index.
    /// The history label names the setting, as in "Change Threshold".
    SetEffectParam {
        track: TrackId,
        effect: EffectId,
        param: u32,
        value: f32,
    },
    /// Replaces every setting of an effect, as when a preset is loaded or a
    /// drag moves two settings at once. Fails when the settings are for
    /// another kind of effect than the slot holds.
    SetEffectParams {
        track: TrackId,
        effect: EffectId,
        params: EffectParams,
    },
    /// Copies an effect with its settings. The copy goes right after the
    /// original. Creates an [`EffectId`]. Fails when the track is full.
    DuplicateEffect {
        track: TrackId,
        effect: EffectId,
    },
    /// Puts a new effect of this kind where an effect is now, as one undo
    /// step. The new effect takes the old one's place in the chain and is
    /// otherwise what [`Command::AddEffect`] makes: switched on, at full
    /// mix, with its default settings and an id of its own. Creates an
    /// [`EffectId`]. Replacing an effect by one of the same kind gives a
    /// fresh one too. The automations of the old effect go with it.
    ReplaceEffect {
        track: TrackId,
        effect: EffectId,
        kind: EffectKind,
    },

    // Playlist
    /// Creates a [`PlaylistTrackId`].
    AddPlaylistTrack {
        #[serde(default)]
        #[ts(optional)]
        /// Defaults to "Track N".
        name: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        /// Position among the playlist tracks. Defaults to the end.
        index: Option<u32>,
    },
    /// Removes the track and its clips.
    RemovePlaylistTrack {
        id: PlaylistTrackId,
    },
    UpdatePlaylistTrack {
        id: PlaylistTrackId,
        patch: PlaylistTrackPatch,
    },
    /// Moves a playlist track, with its clips, to `index` among the
    /// playlist tracks.
    MovePlaylistTrack {
        id: PlaylistTrackId,
        index: u32,
    },
    /// Creates one [`ClipId`] per clip, in order.
    AddClips {
        clips: Vec<ClipInit>,
    },
    RemoveClips {
        clips: Vec<ClipId>,
    },
    /// Changes where clips of any kind sit and what part of their content
    /// they show.
    UpdateClips {
        updates: Vec<ClipUpdate>,
    },
    /// Changes what is particular to audio clips. Fails when one of the
    /// clips is not an audio clip.
    UpdateAudioClips {
        updates: Vec<AudioClipUpdate>,
    },

    // Automation
    /// Adds an automation: a curve for `target`. Creates an
    /// [`AutomationId`]. It plays once a clip puts it on the playlist
    /// ([`ClipContent::Automation`]). Fails when the project has no such
    /// target.
    AddAutomation {
        #[serde(default)]
        #[ts(optional)]
        /// Defaults to a name made from the target, which says what kind
        /// of thing it is: "Kick volume" and "Kick pan" for a channel,
        /// "Kick track volume" and "Kick track pan" for a mixer track,
        /// "Kick to Bus send", "Bus Reverb mix" and "Bus Reverb Decay" for
        /// an effect on the track Bus ("Bus Reverb 2 Decay" for the second
        /// reverb of that track), "Lead Cutoff" for a setting of the
        /// instrument on the channel Lead, and "Tempo". A name that an
        /// automation already has is numbered: "Kick pan 2".
        name: Option<String>,
        target: AutomationTarget,
        #[serde(default)]
        #[ts(optional)]
        /// The curve, checked as for [`Command::SetAutomationPoints`].
        /// Defaults to one point at tick 0 with the value the target has
        /// now, so that the automation changes nothing until it is drawn.
        points: Option<Vec<AutomationPoint>>,
    },
    /// Removes the automation and every clip that shows it.
    RemoveAutomation {
        id: AutomationId,
    },
    UpdateAutomation {
        id: AutomationId,
        patch: AutomationPatch,
    },
    /// Replaces the whole curve of an automation. Values are brought into 0
    /// to 1 and curves into -1 to 1. Fails when there is no point, when
    /// there are more than
    /// [`MAX_AUTOMATION_POINTS`](crate::model::MAX_AUTOMATION_POINTS), when
    /// the points are not in order of their ticks, when a tick is past
    /// [`MAX_SONG_TICKS`](crate::model::MAX_SONG_TICKS), and when a value
    /// or curve is not a number. Dispatched with a gesture id, the drag of
    /// a point is one undo step.
    SetAutomationPoints {
        id: AutomationId,
        points: Vec<AutomationPoint>,
    },
    /// Write a bounded musical LFO into a captured curve range in one edit.
    /// The complete automation must still match, including target and points.
    GenerateAutomationLfo {
        expected: crate::Automation,
        start: u32,
        end: u32,
        resolution: u32,
        lfo: crate::CurveLfo,
    },
    /// Copies the automation, with its target and curve but not its clips.
    /// The copy goes right after the original. Its name is the original's
    /// with a number, as in "Tempo #2". Creates an [`AutomationId`].
    DuplicateAutomation {
        id: AutomationId,
    },

    /// Runs several commands as one undo step. If any of them fails, none of
    /// them is applied.
    Batch {
        #[serde(default)]
        #[ts(optional)]
        /// History label. Defaults to the label of the first command.
        label: Option<String>,
        commands: Vec<Command>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SettingsPatch {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub author: Option<String>,
    #[ts(optional)]
    pub genre: Option<String>,
    #[ts(optional)]
    pub comments: Option<String>,
    #[ts(optional)]
    pub tempo_bpm: Option<f64>,
    #[ts(optional)]
    pub time_signature: Option<TimeSignature>,
    #[ts(optional)]
    pub swing: Option<f32>,
    #[ts(optional)]
    pub pan_law: Option<crate::PanLaw>,
    /// One of [`TIMEBASES`](crate::timebase::TIMEBASES); another value
    /// fails the command.
    #[ts(optional)]
    pub timebase_ppq: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelPatch {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    /// Empty moves the channel to the ungrouped category.
    pub group: Option<String>,
    #[ts(optional)]
    pub timing: Option<crate::ChannelTiming>,
    #[ts(optional)]
    pub color: Option<u32>,
    #[ts(optional)]
    pub volume: Option<f32>,
    #[ts(optional)]
    pub pan: Option<f32>,
    #[ts(optional)]
    pub muted: Option<bool>,
    #[ts(optional)]
    pub solo: Option<bool>,
    #[ts(optional)]
    pub mixer_track: Option<TrackId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SamplerPatch {
    #[ts(optional)]
    pub stretch: Option<crate::SamplerStretch>,
    #[ts(optional)]
    pub root_key: Option<u8>,
    #[ts(optional)]
    pub tune: Option<f32>,
    #[ts(optional)]
    pub gain: Option<f32>,
    #[ts(optional)]
    pub start: Option<f32>,
    #[ts(optional)]
    pub end: Option<f32>,
    #[ts(optional)]
    pub reverse: Option<bool>,
    #[ts(optional)]
    pub loop_mode: Option<crate::SamplerLoopMode>,
    #[ts(optional)]
    pub loop_start: Option<f32>,
    #[ts(optional)]
    pub loop_end: Option<f32>,
    #[ts(optional)]
    pub loop_crossfade: Option<f32>,
    #[ts(optional)]
    pub cut_self: Option<bool>,
    #[ts(optional)]
    pub cut_group: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PatternPatch {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub color: Option<u32>,
    #[ts(optional)]
    pub length_steps: Option<u32>,
}

/// A note to add. Missing fields take the defaults a step would get.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteInit {
    pub start: u32,
    pub length: u32,
    pub key: u8,
    #[serde(default)]
    #[ts(optional)]
    pub velocity: Option<f32>,
    #[serde(default)]
    #[ts(optional)]
    pub pan: Option<f32>,
    #[serde(default)]
    #[ts(optional)]
    pub expression: Option<crate::NoteExpression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoteUpdate {
    pub id: NoteId,
    pub patch: NotePatch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NotePatch {
    #[ts(optional)]
    pub start: Option<u32>,
    #[ts(optional)]
    pub length: Option<u32>,
    #[ts(optional)]
    pub key: Option<u8>,
    #[ts(optional)]
    pub velocity: Option<f32>,
    #[ts(optional)]
    pub pan: Option<f32>,
    #[ts(optional)]
    pub expression: Option<crate::NoteExpression>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MixerTrackPatch {
    #[ts(optional)]
    pub dock: Option<crate::MixerDock>,
    #[ts(optional)]
    pub processing: Option<windfall_dsp::TrackParams>,
    #[ts(optional)]
    pub latency_offset_ms: Option<f64>,
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub color: Option<u32>,
    #[ts(optional)]
    pub volume: Option<f32>,
    #[ts(optional)]
    pub pan: Option<f32>,
    #[ts(optional)]
    pub muted: Option<bool>,
    #[ts(optional)]
    pub solo: Option<bool>,
    #[ts(optional)]
    pub recording: Option<crate::MixerRecording>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct EffectSlotPatch {
    #[ts(optional)]
    pub enabled: Option<bool>,
    #[ts(optional)]
    pub mix: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PlaylistTrackPatch {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub muted: Option<bool>,
    #[ts(optional)]
    pub solo: Option<bool>,
    #[ts(optional)]
    pub color: Option<u32>,
    #[ts(optional)]
    pub height: Option<u32>,
}

/// A clip to add.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipInit {
    pub track: PlaylistTrackId,
    pub start: u32,
    /// Defaults to the natural length of the content: one pass of a
    /// pattern, or an automation's curve up to its last point and at least
    /// one bar. An audio clip has to be given its length, because how long
    /// its audio lasts is not something the project knows. The shell's
    /// `add_audio_clip_from_file` and `add_audio_clip_from_sample` work it
    /// out from the file.
    #[serde(default)]
    #[ts(optional)]
    pub length: Option<u32>,
    /// How far into its content the clip starts, in ticks. Defaults to 0.
    #[serde(default)]
    #[ts(optional)]
    pub offset: Option<u32>,
    /// Defaults to false.
    #[serde(default)]
    #[ts(optional)]
    pub muted: Option<bool>,
    /// What the clip plays. The values of an audio clip are brought into
    /// their ranges like those of any command.
    pub content: ClipContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipUpdate {
    pub id: ClipId,
    pub patch: ClipPatch,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AutomationPatch {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub color: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioClipUpdate {
    pub id: ClipId,
    pub patch: AudioClipPatch,
}

/// The fields of [`ClipContent::Audio`], which says what each one means.
/// The sample of a clip cannot be changed: a clip of another sample is
/// another clip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AudioClipPatch {
    #[ts(optional)]
    pub mixer_track: Option<TrackId>,
    #[ts(optional)]
    pub output: Option<crate::ClipAudioOutput>,
    #[ts(optional)]
    pub gain: Option<f32>,
    #[ts(optional)]
    pub normalize: Option<bool>,
    #[ts(optional)]
    pub pan: Option<f32>,
    #[ts(optional)]
    pub fade_in: Option<u32>,
    #[ts(optional)]
    pub fade_out: Option<u32>,
    #[ts(optional)]
    pub reverse: Option<bool>,
    #[ts(optional)]
    pub pitch: Option<f32>,
    #[ts(optional)]
    pub stretch: Option<crate::ClipStretch>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ClipPatch {
    #[ts(optional)]
    pub track: Option<PlaylistTrackId>,
    #[ts(optional)]
    pub start: Option<u32>,
    #[ts(optional)]
    pub length: Option<u32>,
    #[ts(optional)]
    pub offset: Option<u32>,
    #[ts(optional)]
    pub muted: Option<bool>,
}
