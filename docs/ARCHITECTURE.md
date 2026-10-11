# Windfall architecture

This is the working contract between the parts of Windfall. `WINDFALL_PLAN.md` says what gets built and why; this file says where code lives and how the parts talk. If code and this file disagree, fix one of them in the same change.

## Layout

```
Cargo.toml                 Rust workspace
crates/
  windfall-core/           AudioBuffer, PPQ, gain and pan helpers. No dependencies.
  windfall-project/        Project model, Command, Document (undo history), .windfall file format.
  windfall-ipc/            Runtime types the engine, shell and UI exchange (transport, meters, devices, browser, export).
  windfall-flp/            Bounded FL Studio project reader, checked conversion and import reports.
  windfall-codec/          Decode and encode WAV, FLAC, MP3, OGG. Waveform overviews.
  windfall-midi/           Standard MIDI File reader/writer, import plans and playlist export.
  windfall-engine/         Realtime audio engine, offline renderer, soak-test CLI.
  windfall-dsp/            Effects and instruments: the internal plugin interface, shared DSP blocks,
                           EQ, compressor, limiter, reverb, delay and a subtractive synth.
  windfall-plugin-host/    CLAP effects/instruments, isolated CLAP/VST3 scanner, state and native editors.
  windfall-stretch/        Streaming/offline time-stretch, independent pitch-shift and loop tempo estimation.
  windfall-factory/        Generates the CC0 factory drum samples.
  windfall-sim/            The Document behind a C ABI, compiled to WebAssembly for the UI's mock backend.
content/factory/           Factory content shipped with the app (generated, checked in).
apps/desktop/              The app.
  src-tauri/               Tauri 2 shell. Owns the Document and the Engine. Crate name windfall-desktop.
  src/                     React 19 + Tailwind v4 + shadcn UI.
    bindings/              TypeScript types generated from Rust. Never edit by hand.
    components/ui/         shadcn components.
    components/audio/      Windfall's audio control kit (knob, fader, meter, step button...). Published as a shadcn registry.
    features/              One folder per window or panel.
      params/              The controls of every built-in instrument and effect editor, made from the descriptors.
    lib/ipc/               The Backend interface, its Tauri implementation and an in-browser mock.
      sim/                 What the mock is made of, with windfall_sim.wasm, the built windfall-sim (checked in).
    lib/store/             UI state.
docs/                      This file, the parity matrix, user docs.
scripts/                   msvc-env.sh, gen-bindings.sh, build-sim.sh, check-sim.mjs, parity.mjs.
```

Dependency direction: `core` ← `dsp` ← `project` ← `ipc` ← `engine`; `core` ← `codec`; the shell depends on all of them. `dsp` owns the parameter structs of every effect and instrument, and `project` stores them. The engine never depends on the codec: it is handed decoded audio.

`windfall-project::slicer` is the shared worker/control-side beat-grid and transient detector and linked-source slice command builder. The native `slice_analyze` / `slice_apply` / `slice_discard` IPC commands own one reviewed document/source snapshot. Browser analysis runs the same Rust detector in a dedicated WASM worker. `features/slicer` provides marker review from the playlist audio inspector. Apply is one undoable batch; generation/edit/source/token checks and recording exclusion guard publication after off-lock analysis and plan preparation. See `SLICER.md` for supported settings and scope.

## Building

On Windows, run `source scripts/msvc-env.sh` in Git Bash before any `cargo` command. It loads a Visual Studio environment that has the x64 C++ libraries. Without it, linking can fail.

- `cargo test --workspace` runs the Rust tests.
- `cargo clippy --workspace --all-targets -- -D warnings` must pass.
- `scripts/gen-bindings.sh` regenerates `apps/desktop/src/bindings` after any change to a type that derives `TS`.
- `scripts/build-sim.sh` rebuilds the mock backend's WebAssembly document after any change to `windfall-sim`, `windfall-project` or `windfall-core`. `node scripts/check-sim.mjs` fails when it is stale.
- `pnpm --dir apps/desktop dev` runs the UI in a browser against the mock backend.
- `pnpm --dir apps/desktop tauri dev` runs the real app.

## Time and units

- Musical time is in ticks, 960 per quarter note (`PPQ`). A step is a sixteenth note, 240 ticks (`TICKS_PER_STEP`).
- A pattern is at most 1,024 steps long (`MAX_PATTERN_STEPS`, 245,760 ticks, `MAX_PATTERN_TICKS`). A song is at most a million quarter notes long (`MAX_SONG_TICKS`, 960,000,000 ticks: 250,000 bars of 4/4, about 139 hours at 120 bpm). That is under a quarter of what a `u32` holds, so a start, a length and an offset add up without overflowing.
- Gain is linear. 1.0 is 0 dB. Faders stop at 2.0 (`MAX_GAIN`).
- Pan runs from -1 (left) to 1 (right) and uses the balance law in `windfall_core::pan_gains`.
- Colors are `0xRRGGBB` integers.
- JSON is camelCase everywhere.

## Project model and commands (`windfall-project`)

`model.rs` defines `Project` and everything in it. `command.rs` defines `Command`. Read those two files; their doc comments are the specification of each field and command.

- Ids are `u32` newtypes allocated from `Project::next_id`. They are never reused. Id 0 is the master mixer track.
- The step sequencer and the piano roll edit the same notes. A lit step is a note at `step * 240` ticks with key 60 and length 240.
- Adding a channel also adds a mixer track for it, named after it, and routes the channel there. This is the plan's automatic routing. Renaming the channel renames that track in the same undo step for as long as the track is the channel's own and has the channel's name: nothing else plays into it (no other channel, no audio clip, no other track), and the user has not given it another name.
- A channel is a sampler or an instrument: `ChannelSource` is `{ "type": "sampler", ... }` or `{ "type": "instrument", "params": InstrumentParams }`. The sampler commands fail on an instrument channel and the instrument commands on a sampler.
- Every mixer track, the master included, has `effects: EffectSlot[]`, at most 10 (`MAX_EFFECT_SLOTS`), in the order the signal passes through them. A slot is `{ id: EffectId, enabled, mix, params: EffectParams }`. An `EffectId` is unique in the project and stays with the effect when it moves to another track.
- `EffectParams` and `InstrumentParams` are `windfall-dsp`'s tagged unions (`type` is the kind, such as `"reverb"` or `"subtractiveSynth"`), and the project stores them with every value inside its range. `EffectKind::descriptors()` and `InstrumentKind::descriptors()` list the settings of a kind. `SetEffectParam` and `SetInstrumentParam` address one setting by its index in that list, which is what knobs and, later, automation use. `SetEffectParams` and `SetInstrumentParams` replace all of them.
- Effect edits touch the `mixer` section of a patch and instrument edits the `channels` section. Both merge under a gesture id like any other drag. The history names the setting a knob moved: "Change Cutoff", "Change Threshold". `ReplaceEffect { track, effect, kind }` puts a new effect, with a new id, in the place of an old one as one undo step.
- A clip on the playlist plays one of the kinds of `ClipContent`: `{ "type": "pattern", pattern }`, `{ "type": "audio", sample, mixerTrack, gain, pan, fadeIn, fadeOut, reverse, pitch }` or `{ "type": "automation", automation }`. `start`, `length`, `offset` and `muted` belong to every clip. `AddClips` takes `offset` and `muted` too, and an audio clip has to be given its `length`, which the shell works out from the file. `UpdateClips` moves and trims clips of any kind; `UpdateAudioClips` changes what only audio clips have. A sample cannot be removed while an audio clip plays it, and removing a mixer track sends its audio clips to the master.
- The playlist's tracks are ordered: `AddPlaylistTrack` takes an `index`, and `MovePlaylistTrack { id, index }` moves a track with its clips.
- No edit puts a note at or past `MAX_PATTERN_TICKS`, where it could never play, and no clip ends past `MAX_SONG_TICKS`. Both fail the command with a message. `Project::check` enforces the clip rule, so no file breaks it; the note rule is the commands' alone, and a note that an older file has further out loads and can be edited and brought back.

- `Project.automations` holds the automation curves: `{ id, name, color, target, points }`. A file from before automation has no such field and loads with none. An automation plays where a clip puts it; several clips can show one automation, and several automations can have one target. `AddAutomation`, `RemoveAutomation`, `UpdateAutomation` (name and color), `SetAutomationPoints` (the whole curve, which merges under a gesture id like any drag) and `DuplicateAutomation` edit them, and touch the `automations` section of a patch.
- An automation added without a name is named after its target, in words that tell the kinds of target apart: "Kick volume" and "Kick pan" for a channel, "Kick track volume" and "Kick track pan" for a mixer track, "Kick to Bus send", "Bus Reverb mix" and "Bus Reverb Decay" for an effect with the name of its track ("Bus Reverb 2 Decay" for the second reverb in that track's chain), "Lead Cutoff" for a setting of a channel's instrument, and "Tempo". If an automation already has the name, capitals aside, the new one is numbered: "Kick pan 2".
- An automation lives as long as what it moves. Removing a channel, a mixer track, an effect or a send, and replacing an effect, removes the automations of it and their clips in the same undo step. An effect that moves to another track takes its automations along.

#### Automation

A point of a curve is `{ tick, value, curve, hold }`: ticks from the start of the curve, a value from 0 to 1 across the target's range, the bend of the stretch that leaves the point (-1 to 1, 0 is straight) and whether the curve steps instead. `windfall_project::automation` has the two pure functions everything else uses, `curve_value(points, tick)` and `AutomationRange::value(normalized)` with its inverse, and `Project::automation_range(target)` gives the range of a target. The engine uses them, and they are part of `windfall-project`, which `windfall-sim` is built from, so a UI that draws a curve in real units can be handed the same code. The table below is what they compute.

| Target | Range | A value `n` of 0 to 1 means |
|---|---|---|
| `channelVolume`, `trackVolume`, `sendGain` | linear gain 0 to 2 | `2 * n * n`: 0 is silence, 0.7071 is 0 dB, 1 is +6 dB |
| `channelPan`, `trackPan` | -1 to 1 | `2 * n - 1` |
| `effectMix` | 0 to 1 | `n` |
| `tempo` | 10 to 522 bpm | `10 + 512 * n`, so 120 bpm is 0.21484375 |
| `effectParam`, `instrumentParam` | the descriptor's `min` to `max` | linear, or `min * (max / min)^n` where the descriptor's scale is logarithmic; a whole number or a choice rounds to the nearest step, a toggle is on from 0.5 up |

A bent stretch follows `(e^(k * t) - 1) / (e^k - 1)` with `k = 6 * curve` and `t` the share of the stretch that has passed: a positive curve holds back and catches up, a negative one hurries and settles. An automation has at most 4,096 points.

While the song plays, what a target does depends on the place in the song alone:

- Inside an automation clip it follows the curve. The clip is a window: tick `start` of the song is tick `offset` of the curve.
- After a clip it holds the value the curve has at the clip's end, until another clip of that target begins. After its last clip it holds to the end of the song.
- Before the first clip of a target, and in pattern mode, it has the value stored in the project.
- Where clips of one target overlap, the clip on the playlist track nearest the top decides, then the one that starts later, then the one with the lower id. Muted clips, and clips on muted playlist tracks, count for nothing.

Playback that begins in the middle of a song therefore finds every target where the song would have it. A looping song plays through its end: each time around starts from the values that playing from the start gives, and a target whose first clip lies further on is back at its stored value 5 ms after the loop point.

A song that stops keeps its automation until it has rung out:

- When the song reaches its end by itself, or Stop is pressed while it plays, every automated target stays at the value it had there for as long as anything is still sounding: a voice, an audio clip's fade, an instrument, or a track whose effects have not rung out. This is the test that ends an automatic tail (see Tails). A master fader that a curve has brought down to silence therefore keeps the reverb's tail silent, and a tail is heard at the reverb mix the song ended on.
- Once nothing is sounding, the targets return to their stored values over 100 ms.
- They return sooner, over the same 100 ms, when the stopped transport is used or something is played by hand: Stop again, a seek, a change of mode, a note or a preview. What is played then is heard at the stored values, and a tail that is still ringing swells or sinks to them without a jump.
- Play ends the hold too. The targets the song has a value for at the place it starts from are there on the first frame, and the others return over 5 ms.
- An edit made during the hold leaves it as it is.
- A gain or a pan returns in a straight line. A setting of an effect or instrument, and an effect's mix, is walked back in steps of 64 frames, evenly across the automation's own 0 to 1, with its processor gliding from step to step.
- An offline render never lets go: its tail is rendered to the last frame at the values the song ended on.

Automation is never written into the project: the stored value is what a control shows when no automation has it in hand.

#### Audio clips and time

An audio clip starts on its `start` tick and then runs in seconds, not in beats: `2^(pitch / 12)` seconds of the file for every second of the song, at the file's own sample rate. `pitch` is a tape speed by default. An absent `stretch` field loads as `{ mode: "tape" }`. `{ mode: "spectral", ratio, quality, formants }` prepares the whole original source with an independent pitch (±24 semitones), duration multiplier (0.25–4), quality (`fast`, `standard`, `high`) and optional formant preservation. Spectral audio plays at unity speed; its source position uses `1 / ratio` in place of the tape speed below.

- `offset` skips the start of the audio by as much as would have played in that many ticks at the project's stored tempo: `offset * 60 / (tempoBpm * 960) * 2^(pitch / 12)` seconds of the file. Trimming a clip's left edge (`start + n`, `offset + n`, `length - n`) therefore leaves the rest of the audio where it was. A reversed clip skips from the end of the file.
- The clip ends on tick `start + length`, or sooner if the audio runs out. `length` only ever cuts. A clip that ends exactly with its audio is `duration / 2^(pitch / 12) * tempoBpm * 16 - offset` ticks long.
- A tempo change leaves `start`, `length` and `offset` alone. The clip starts on the same beat and its audio takes the same seconds, so a faster tempo makes `length` cut more of its end, and a slower one lets it end before the clip does.
- `fadeIn` and `fadeOut` are lengths in ticks from the clip's start and before its end, shaped as equal-power curves.

The playlist inspector prepares spectral settings using `prepare_clip_command` on a blocking worker. It snapshots the document and pool, validates the command, renders without the session state lock, then rejects installation after document edits, replacement or source reload. Playback and export compile the same immutable prepared buffers. A shared cache retains up to 32 variants or 256 MiB (one oversized variant may exceed this); source identity invalidates reloads. The worker compiles a complete immutable plan before installation, so evicting cache entries cannot cause another render under the session lock during Apply. Project opening and history restoration likewise compile target snapshots before taking the lock. Other session publications test for cache misses and coalesce preparation onto a worker, leaving the previous plan audible until the current snapshot is ready. Source, generation and edit guards prevent stale plans from being installed. DSP never runs with the session state lock held or on the audio callback. Spectral clips keep the existing tick-based trimming, reverse and fade rules. Applying a duration change rescales length, offset and fades together to keep the same original source interval in one undoable batch. Pitch changes within spectral mode leave duration unchanged.

`detect_clip_tempo` analyzes the whole source off-thread and returns periodicity candidates with scores, including half/double ambiguity; an empty result asks for explicit source BPM or the whole file's beat count. Fitting sets a fixed ratio to the current project tempo and does not enable automatic following of future tempo changes. Browser simulation supports reversible settings but has no audio DSP or tempo detection. Samplers remain tape playback.

`Document` owns the one copy of the `Project` plus the undo history.

The basic [audio editor](AUDIO-EDITOR.md) snapshots a selected clip into a bounded immutable stereo view using playback's source reader, spectral preparation and explicit fade law. Worker IPC renders edits into new app-owned WAVs, then checks recording exclusion, generation, edits and source identity before attaching a derived sample/clip in one undoable batch. Originals are never overwritten. Tempo-automated projects and browser file operations are explicitly refused in this first editor.

```rust
impl Document {
    pub fn new(project: Project) -> Self;
    pub fn project(&self) -> &Project;
    pub fn revision(&self) -> u64;
    /// Applies a command and records it for undo. Consecutive dispatches that
    /// carry the same non-None gesture id collapse into one undo step (a fader drag).
    pub fn dispatch(&mut self, command: Command, gesture: Option<u64>) -> Result<Applied, CommandError>;
    pub fn undo(&mut self) -> Option<Touched>;
    pub fn redo(&mut self) -> Option<Touched>;
    /// Undoes or redoes until `cursor` entries are applied.
    pub fn jump(&mut self, cursor: u32) -> Touched;
    pub fn history(&self) -> HistoryView;
    pub fn is_dirty(&self) -> bool;
    pub fn mark_saved(&mut self);
    /// Changes where a sample's file is stored, in the project and in every stored
    /// undo and redo step that holds the sample. Not an edit: no history step, and
    /// the dirty state stays as it is.
    pub fn relink_sample(&mut self, id: SampleId, path: SamplePath) -> Result<Touched, CommandError>;
    /// Builds the patch for the UI and bumps the revision.
    pub fn patch(&mut self, touched: &Touched) -> ProjectPatch;
    pub fn snapshot(&self, path: Option<String>) -> DocumentSnapshot;
}
pub struct Applied { pub created: Vec<u32>, pub touched: Touched, pub label: String }
```

A failed command leaves the project exactly as it was. Undo restores every entity exactly, including its id, and redo restores it exactly again. The one thing undo does not roll back is `next_id`: an id never comes to mean something else for the life of a document, so caches and selections keyed by id stay valid.

Rules the document enforces beyond the type shapes: lanes are sorted by channel id, the master track has no sends, a mixer that is already full routes a new channel to the master, and the settings of effects and instruments are stored inside their ranges.

`relink_sample` exists for "Save as" into another folder, which may have to give a sample's file a new name there. Undoing a rename would point the project at the other file of that name, so it is not an undo step. It fails when another sample, in the project or anywhere in the history, already has the path.

### File format

A project is one readable JSON file with the extension `.windfall`, pretty-printed, holding the `Project`. A file from before mixer tracks had effects has no `effects` field and loads with none.

Beside the project's own keys the file may hold one more, `session`, a `ProjectSession`: `{ "mode": "song", "loopSong": true, "pattern": 12 }`. It says how the project was being played when it was saved, so that a song opens in song mode with its looping and its selected pattern. It is the transport's state and not the project's. `Project` has no field for it, so it is in no patch, in no undo step and in no comparison of two projects, and changing the mode does not make a project unsaved. The shell reads it off the transport when it saves or writes a backup (`file::save_with`, `file::write_backup_with`) and hands it back to the transport when it opens the file (`file::load_with_session`), and the UI hears of it as the `transport:state` event that follows every load. A file without the key opens in pattern mode on its first pattern, as every file did before; a `session` that cannot be read, or that names a pattern the project does not have, is ignored as far as it cannot be used. `file::save`, `file::load` and `file::to_json` write and read a file without it. It lives in a project folder. Samples copied into the project sit beside it and are stored as `SamplePath::Project` paths relative to that folder. Backups are written to `Backup/<name> <timestamp>.windfall` in the same folder.

Project files, backups and the app's settings file are all written by `windfall_project::file::write_atomic`: to a temporary file of that write's own (`.<name>.<pid>-<serial>.tmp`) that is then renamed into place. A crash cannot leave half a file, two writers cannot write into each other's, and the file that is there is never damaged. Windows refuses to replace a file another program holds open, as a virus scanner does for a moment; such a rename is tried six times over a third of a second before the write fails. Saving a project also deletes temporary files of that shape in its folder that are more than a day old, which only a write that died can have left.

## Engine (`windfall-engine`)

The audio thread never allocates, locks, or blocks. Everything it needs arrives ready-made.

- The control side compiles the `Project` plus a `SamplePool` (decoded `AudioBuffer`s by `SampleId`) into an immutable plan and sends it to the audio thread over a lock-free queue. The audio thread swaps plans at a buffer boundary and sends the old one back to be freed off-thread.
- Voice state, transport position and meters live on the audio thread. Meters and the playhead are published through atomics.
- `Processor` is the whole audio path with no device attached: `process(&mut [f32] interleaved stereo)`. The device callback calls it, the offline renderer calls it in a loop, and tests call it directly. Export therefore matches playback.
- The sampler resamples by ratio, so a sample at any rate plays in tune at any device rate. A sampler envelope's attack is a straight line, and its decay and release are exponential curves that land exactly when their time is up.
- Previews play at -6 dB (`PREVIEW_GAIN_DB`), so one heard over a full mix does not clip the master.
- `Engine::reconfigure` carries playback on: the new stream picks up at the playhead. Playback stops only when no stream could be opened.
- `windfall-codec` decodes audio only. It reads no file metadata: tags, loop points and tempo markers in a file are ignored. A file whose header claims more than 64 channels or a sample rate above 768 kHz is refused before any decoder is made, and no more than 1 MiB is set aside on a header's word: the buffer grows as audio arrives.

```rust
pub struct SamplePool;                    // insert(SampleId, AudioBuffer), remove, get, contains,
                                          // ids(), iter(), retain(keep); cheap to clone
pub struct Engine;                        // Send + Sync; the cpal stream lives on a thread it owns
impl Engine {
    pub fn start(settings: &AudioSettings) -> Engine;   // never fails: with no device, status().running is false and error says why
    pub fn reconfigure(&self, settings: &AudioSettings);
    pub fn status(&self) -> EngineStatus;
    pub fn devices() -> Vec<AudioHost>;
    pub fn controller(&self) -> Controller;
}
#[derive(Clone)] pub struct Controller;   // Send + Sync
impl Controller {
    pub fn set_project(&self, project: &Project, pool: &SamplePool);
    pub fn play(&self); pub fn stop(&self); pub fn seek(&self, tick: f64);
    pub fn set_transport(&self, patch: TransportPatch);
    pub fn transport(&self) -> TransportState;
    pub fn note_on(&self, channel: ChannelId, key: u8, velocity: f32);
    pub fn note_off(&self, channel: ChannelId, key: u8);
    pub fn preview(&self, sample: AudioBuffer); pub fn stop_preview(&self);
    pub fn frame(&self) -> RealtimeFrame;  // resets the meter peaks it reads
    pub fn latency_frames(&self) -> u32;   // what the project's effects and instruments add
}
pub fn render(project: &Project, pool: &SamplePool, options: &RenderOptions,
              progress: &mut dyn FnMut(f32) -> bool) -> AudioBuffer;
/// The same, with what to tell the listener about it.
pub fn render_reporting(project: &Project, pool: &SamplePool, options: &RenderOptions,
              progress: &mut dyn FnMut(f32) -> bool) -> Rendered;
pub struct Rendered { pub audio: AudioBuffer, pub dropped_clips: u32 }
/// The same audio handed on a block at a time, interleaved stereo, instead of returned.
pub fn render_streaming(project: &Project, pool: &SamplePool, options: &RenderOptions,
              sink: &mut dyn FnMut(&[f32]) -> bool, progress: &mut dyn FnMut(f32) -> bool) -> Streamed;
/// The mix and the mixer tracks apart. `sink` gets the number of a stream and its next frames.
pub fn render_stems(project: &Project, pool: &SamplePool, options: &RenderOptions, stems: &StemOptions,
              sink: &mut dyn FnMut(usize, &[f32]) -> bool, progress: &mut dyn FnMut(f32) -> bool)
              -> Result<Streamed, StemError>;
/// The streams `render_stems` makes, in the order it numbers them, each with a name for a file.
pub fn stems(project: &Project, options: &StemOptions) -> Result<Vec<Stem>, StemError>;
pub struct StemOptions { pub mode: StemMode, pub tracks: Option<Vec<TrackId>>, pub include_mix: bool, pub numbered: bool }
pub struct Stem { pub name: String, pub track: Option<TrackId> }   // track None is the mix
pub struct Streamed { pub frames: u64, pub dropped_clips: u32, pub completed: bool }
```

### Effects and instruments

The signal on a mixer track: everything that plays into it, summed, then its effects in chain order, then its fader and pan, and from there its meter, its output and its sends. Effects are inserts before the fader, on the master as on any track.

Effects and instruments are `windfall-dsp` processors, and the engine hosts both the same way:

- The control side builds and prepares a processor, in `set_project`, when the plan needs one the audio thread does not have, and when a new stream opens. That allocates; the audio thread never does. The processor goes to the audio thread with the plan.
- An effect is known by its `EffectId` and an instrument by its `ChannelId`. For as long as the project keeps the id, the same processor keeps running from plan to plan with all it remembers: a reverb tail survives a fader move, a new note, a changed setting, and the effect being moved to another track.
- New settings arrive with the plan and are handed to the processor at the start of a buffer. The processor glides to them. `enabled` and `mix` are the dsp slot's, which crossfades over 10 ms. The tempo is passed on when it changes.
- A processor the project no longer has travels back to the control side and is dropped there.

What sound is passing through changes gently:

- An effect that joins a track with sound on it fades in over 5 ms. One with latency first runs unheard for as long as that latency.
- An effect that leaves the project fades out over 5 ms, and an instrument whose channel is gone stops with a 4 ms fade. Both stay in the plan for one more `set_project` to do so.
- A sounding sampler voice whose channel moves to another mixer track crossfades between the two over 4 ms.
- A voice that is fading out when the plan changes, or that loses its channel, finishes inside the track it was in whenever an effect or a compensation delay lies between that track and the output, so it is heard through those effects and shows on the track's meter. With nothing but faders in the way it leaves the mix and finishes at the output at the level it had, which lets the new faders take their values at once. If the track is gone as well, it finishes at the output at its fader gains.
- These switch at once: effects that trade places within one chain, the track an effect was on when it moves to another track, an instrument channel that moves to another track, and a removed track's own effects.

Sequenced notes reach an instrument on their exact frame and are let go on their tick, so a tempo change moves the end of a held note. The velocity of a note is the instrument's velocity, and per-note pan places its voice. A key holds one note at a time: of two overlapping notes on one key, the key comes up when the one that started later ends. Stopping fades the instruments out, and seeking ends the pattern's notes and leaves the ones played by hand.

### Audio clips

In song mode an audio clip starts on the first frame at or after its start tick, like a note, and plays into its mixer track where the sampler voices do: through the track's compensation delay, its effects, its fader and its sends.

- Playback that begins inside a clip, after a seek or because a clip was unmuted, moved or added under the playhead, begins at the frame of the audio that belongs there.
- The audio is read with the sampler's interpolation, so a file at any rate plays in tune at any device rate, and at whole-number speeds its frames come out untouched.
- A clip that starts on the first frame of its audio plays that frame untouched, as the sampler plays a one-shot, so a drum loop that begins on a hit keeps the hit. Every other start and every stop is a cut, and is faded over 3 ms: a clip that begins part way into its audio (an `offset`, a reversed clip, playback that begins or is moved inside it, a clip that is unmuted or moved under the playhead), where a clip or its audio ends, and where the transport stops, seeks or loops. The clip's own fades are equal-power curves between two ticks, shaped over the time between them, so they follow the tempo as the clip's edges do.
- A tempo change bends the clock under a sounding clip: its audio carries on frame for frame, and its end and fades move to where their ticks now fall.
- An edit to a sounding clip's gain or pan glides over 5 ms. An edit to its start, offset, pitch, direction, sample or mixer track makes it another sound: the old one fades out over 3 ms while the new one fades in at the right place.
- Audio clips play in song mode only, like automation.
- 128 audio clips sound at once (`MAX_AUDIO_CLIPS`). One more is not started and stays silent for its whole length. Clips take their slots as they start, and clips that start on one tick in the order of their ids, so the clips left out are the ones that start last and, among those that start together, the ones with the highest ids. Playback that begins in the middle of a song hands the slots to the clips under the playhead in the same order. The choice depends on the song alone, so a render leaves out what playback from the top leaves out. Clips that are fading out after a stop or a seek do not count; they finish in 128 spare slots.
- A clip that is left out is reported. `RealtimeFrame.droppedClips` counts the clips that would be sounding right now had there been a slot, and `render_reporting` counts every clip a render left out, which the last `export:progress` event passes on as `droppedClips`.
- The slots are allocated once. A clip that outlives every other owner of its sample hands the audio back to the control side to be freed.

### Automation

The control side compiles the automation clips that sound into one lane per target: a row of spans that follow each other, each a stretch of some clip's curve or the value the last clip left. Which clip wins is settled there. The audio thread looks a tick up.

- The lanes are looked at on a fixed grid of 64 frames, counted from the engine's first frame and not from where a buffer begins, so a render is the same for every block size. At each look every target is set on its way to the value it is to have one step on.
- A gain or a pan (channel volume and pan, track fader and pan, send level) is a ramp aimed to arrive exactly one step later. Its course is the curve read every 64 frames and joined by straight lines: there are no steps in it. A setting of an effect or an instrument, and an effect's mix, is handed to the processor, which glides to it as it does when a knob is turned.
- A corner or a jump of a curve, and the edge of a clip, therefore take effect within 64 frames of their tick (1.33 ms at 48 kHz, 0.67 ms at 96 kHz), and a jump is spread over that step. Measured on a 1 kHz sine: a fader swept from silence to +6 dB in a quarter of a second is within 2e-5 of the ideal curve sample for sample, with nothing above -90 dB where stepping at 750 Hz would put tones; an equaliser's gain swept across its whole 48 dB in the same time leaves -93 dB there.
- Where playback begins or is moved to, a gain or pan is at the curve's value from the first frame, so a song that opens on a fade in from silence opens silent.
- A target the song lets go of while it plays, at a loop point or after a seek to before its first clip, returns to its stored value over 5 ms. A song that has stopped holds its targets until it has rung out and then returns them over 100 ms, as the rules under Automation in the project model say. The audio thread asks whether anything still sounds at each look at the lanes, so the frame of the return depends on the audio alone and not on the buffer size.
- A channel or track that mute or solo silences stays silent under a volume curve.
- A setting that changes an effect's latency, the limiter's look-ahead, is not automated: the delays that line the other paths up with it are made for the stored value.
- A new plan carries the lanes on: a target that automation had in hand keeps its place through the edit.
- No lane allocates on the audio thread, and nothing is written back to the plan.

#### Tempo automation

A tempo curve moves the song's own clock. The sequencer's clock stays a straight line from ticks to frames; in song mode it counts *warped* ticks, the tick at which each moment of the song would come at the stored tempo. The plan's tempo map turns a tick of the song into its warped tick and back: it is the stored tempo over the tempo, summed over the ticks on the way, in segments in which the tempo moves in a straight line, where the sum has a closed form (a logarithm). A bent stretch of the curve is followed with straight pieces a sixteenth of a beat long.

- Notes, clip edges, automation and the playhead are all placed through the map, from one anchor, so nothing drifts however the output is divided into buffers. A song whose tempo ramps from 60 to 180 bpm over four bars renders to the frame its sum gives, and every beat on the way lands on its own.
- A note ends on its tick, wherever the map puts that. An audio clip starts and is cut on its ticks and plays at its own speed between them. Its `offset` is counted at the stored tempo.
- Effects and instruments that follow the tempo, such as a synced delay, are told the tempo of the place in the song at every look at the lanes.
- An edit to the tempo curve under a playing song keeps the song's place: the clock is set up anew on that frame, and the ends of sounding notes move with it.
- `RealtimeFrame.tick` is a place in the song, so the playhead moves faster where the tempo is higher.

### Delay compensation

A limiter puts its output out late by its look-ahead, and the synth by 12 samples. The engine delays whatever would otherwise arrive early, so that every path from a note to the output takes the same time.

- A track's input has three kinds of source: its sampler voices together, each instrument that plays into it, and each track that feeds it through its output or a send. Each of them has a delay of its own.
- How far behind a track's input is, is the largest figure among its sources. Every other source is delayed up to it. The track's output is behind by that plus the latency of its effects. An effect that is switched off or half mixed has the same latency as when it is on.
- The master's figure is the latency of the engine: `Controller::latency_frames`, and `EngineStatus.latencyFrames`.
- When a delay has to change, it crossfades to its new length over 5 ms, which is how the limiter moves its own look-ahead, and it waits first if the effect that caused the change is still filling up. Paths stay lined up to the sample while a look-ahead moves, and while a limiter is added or removed.
- Delay lines are sized for the longest look-ahead the effects on the way could be set to and are reused from plan to plan, so nothing is allocated on the audio thread. No path is compensated for more than one second.
- A project with no latency in it has no delay lines and pays nothing.

Every source of every track is compensated, sends included. Two things are not lined up: where the list above says something switches at once, the old and the new path are not crossfaded, and a voice that finishes at the output because its track was removed is not delayed to match the rest.

The playhead in `RealtimeFrame.tick` is taken back by the engine's latency, so it shows the place in the song that is leaving the engine now. The device's own buffer is not taken off. `render` leaves the latency off the front of what it returns, so an export starts where the song starts.

### Tails

`RenderOptions.tail_secs` is how long a render goes on after the end of the pattern or song. With `auto_tail` it becomes an upper bound: the render ends once nothing in the mixer is louder than -90 dBFS any more and the output has stayed below that for 100 ms, right after the last frame that was louder. Its length depends neither on the block size nor on the bound, as long as the bound is long enough for the sound.

"Nothing is louder" is measured, not taken from an effect's settings: every voice, audio clip and instrument has ended, and every track is done. A track is done when what goes into its effects and what comes out of them has been below -90 dBFS for as long as something can still be on its way through them (`Effect::gap_samples`: a delay's echo time, a reverb's pre-delay and longest line, a compressor's or limiter's release), or, failing that, when its input has been quiet for as long as its effects say they can ring (`tail_samples`). Numbers that are not zero but far below hearing, which a reverb puts out for a minute after it has died away, do not keep a render going.

### Streamed renders and stems

`render_streaming` is `render` without the buffer: it hands the audio to a sink a block at a time, and the blocks put end to end are what `render` returns, bit for bit, for every block size. With an automatic tail it keeps back what may turn out to lie past the end, so the sink never gets a frame too many. Export uses it, so a long song is never held in memory.

`render_stems` gives several streams at once: the mix if it is asked for, and one stream for each mixer track chosen. The master is never a stem; its sound is the mix. Every stream is stereo, starts where the song starts, and is exactly as long as the mix, automatic tail included. Voices and audio clips take their slots as they do in the mix, mute and solo are as the project has them, and automation, tempo and tails are those of a plain render: the mix stream is that render, bit for bit. There are two kinds of stem (`StemMode`):

- **Track outputs.** What each track's meter shows: the signal that leaves the track after its effects, fader and pan, before its output and sends. A bus is a stem of its own, holding what was sent to it, processed. A track's stem knows nothing of what a bus or the master does to it later, so the stems add up to the mix only when every track plays straight into a master with no effects at unity; with a reverb on a bus they do not. One pass renders all of them: the processor copies each chosen track's buffer out of the mixer, into memory set aside before the pass, so the audio path allocates nothing.
- **To master.** What each track adds to the mix: the song with only what plays straight into that track sounding (a channel's notes, an audio clip), followed to the output through the track's effects, its sends, its buses and the master's effects and fader. A vocal's stem then carries the vocal's share of the reverb bus. Where everything on the way is linear the stems of all tracks add up to the mix; a compressor or limiter on a bus or the master reacts to each stem alone, as it does when the track is soloed. Every stem needs every bus effect to itself, so no single pass can do this: the song is rendered once for each stem with every other channel and audio clip at no level, and once for the mix, which gives the stems their length. That is `stems + 1` renders, or `stems` when the mix is not wanted and the tail is not automatic.

Lining up: what leaves a track is behind the notes by the track's own figure (`PlanState::behind`, the figure of Delay compensation), and the master by the latency of the engine. Each stream has what it is behind left off its front, so a note on the first tick is on the first frame of the mix and of its stem alike, whatever look-ahead lies on which path. A track that reaches nothing can be further behind than the master, and its stream is filled up with silence at the end.

With no tracks listed, the stems are the tracks that have something to give: for track outputs every track a channel or an audio clip plays into, straight or by way of other tracks, and to the master every track one plays straight into. A channel that plays straight into the master is in the mix alone. `stems` names each stream for a file: the track's name without the characters a path cannot hold, at most 60 characters, "Track 4" for a track with no name, `_` in front of a name Windows keeps for a device, "03 Bass" with `numbered` (the track's place in the mixer), and a number after a name another stream already has, whatever the letter case: "Drums", "Drums 2". The mix is "Mix" and comes first; the tracks follow in mixer order.

## IPC between the shell and the UI

The UI never touches Tauri directly. It calls the `Backend` interface in `src/lib/ipc`, which has a Tauri implementation and an in-browser mock. Types come from `src/bindings`.

The mock does not imitate the document. It runs the real one: `windfall-sim` wraps `Document` and the file format in a small C ABI (JSON in, JSON out), and `scripts/build-sim.sh` compiles it to `src/lib/ipc/sim/windfall_sim.wasm`. So a command, a limit, a history label, an error message or a saved file is the same in a browser, in the UI tests and in the app, and a new command needs no second implementation. Only the shell and the engine are simulated in TypeScript: files in the browser's storage, dialogs, the sample browser, the transport and the meters. The module is checked in, so `pnpm dev`, `pnpm test` and the UI job in CI need no Rust. After a change to `windfall-sim`, `windfall-project` or `windfall-core`, run `scripts/build-sim.sh` (once: `rustup target add wasm32-unknown-unknown`) and commit `windfall_sim.wasm` and `windfall_sim.wasm.json`; `node scripts/check-sim.mjs` compares a hash of those sources with the one recorded at the last build. The mock, and with it the module, is loaded only outside Tauri, as a chunk of its own, so the app does not carry it.

Tauri commands. Arguments are camelCase. A failed call rejects with a plain string message.

| Command | Arguments | Returns |
|---|---|---|
| `document_snapshot` | | `DocumentSnapshot` |
| `dispatch` | `command: Command`, `gesture?: number` | `DispatchResult` |
| `undo`, `redo` | | `ProjectPatch \| null` |
| `history_jump` | `cursor: number` | `ProjectPatch` |
| `project_new` | | `DocumentSnapshot`. Rejects when it was overtaken by another new or open, or by an edit. |
| `project_open` | `path: string` | `DocumentSnapshot`. Rejects like `project_new`. A backup (a file in a project's `Backup` folder) opens as a copy: the snapshot's `path` is `null` and a `project:warnings` line says so. |
| `project_save` | `path?: string` | `string`, the saved path. Rejects when there is no path yet. May emit `project:loaded` (see below). |
| `recent_projects` | | `string[]` |
| `transport_play`, `transport_toggle` | | `TransportState`. Reject in song mode when nothing on the playlist would make a sound, with a message for each case: the playlist is empty, every clip is muted or on a muted track, or the clips that are not are all automation clips. |
| `transport_stop` | | `TransportState` |
| `transport_seek` | `tick: number` | |
| `transport_set` | `patch: TransportPatch` | `TransportState` |
| `transport_state` | | `TransportState` |
| `realtime_subscribe` | `channel: Channel<RealtimeFrame>` | Frames arrive about 60 times a second. |
| `engine_status` | | `EngineStatus` |
| `engine_devices` | | `AudioHost[]` |
| `engine_configure` | `settings: AudioSettings` | `EngineStatus` |
| `engine_settings` | | `AudioSettings`: the output the user asked for, as stored. A field left to the system arrives as `null` (Rust writes `None` that way, although the generated type says the field is left out), and the UI takes `null` and a missing field alike to mean "the default". What the engine made of it is in `EngineStatus`. |
| `audition_note_on` | `channel: ChannelId`, `key: number`, `velocity: number` | |
| `audition_note_off` | `channel: ChannelId`, `key: number` | |
| `preview_play` | `path: string` | |
| `preview_stop` | | |
| `browser_roots` | | `BrowserRoot[]` |
| `browser_add_root`, `browser_remove_root` | `path: string` | `BrowserRoot[]` |
| `browser_list` | `path: string` | `BrowserEntry[]`, folders first, then by name |
| `sample_info` | `path: string` | `SampleInfo` |
| `sample_info_by_id` | `sample: SampleId` | `SampleInfo` of a sample in the project's pool. Rejects when the file is missing, and when another project was opened meanwhile. |
| `audio_editor_open` | `clip: ClipId` | `AudioEditPreview`: token, clip/name, frame count, sample rate and waveform of the rendered clip view. Blocking worker, refused during recording. |
| `audio_editor_apply` | `request: AudioEditRequest` | `DispatchResult`: derived sample, then resulting clip in `created`. One undo step; refuses invalid/expired/stale work or recording. |
| `audio_editor_discard` | `token: number` | Releases the retained view only if the token still owns it; blocking worker. |
| `add_channel_from_file` | `path: string`, `index?: number` | `DispatchResult`. One undo step: adds the sample to the pool and a channel that plays it. Rejects when another project was opened meanwhile. |
| `set_channel_sample_from_file` | `channel: ChannelId`, `path: string` | `DispatchResult`. Rejects like `add_channel_from_file`. |
| `samples_reload` | | `number`: how many samples still have no audio after looking for the missing files again. Which ones arrives as `project:warnings`. |
| `automate` | `target: AutomationTarget` | `DispatchResult`. One undo step, "Create automation clip": an automation of the target with one point at the value it has now, a new playlist track at the end, and a clip of the automation on it from tick 0 for the length of the song, at least four bars. `created` holds the automation, the playlist track and the clip, in that order. Rejects a target the project does not have. |
| `add_audio_clip_from_file` | `path: string`, `track?: PlaylistTrackId`, `start: number`, `mixerTrack?: TrackId` | `DispatchResult`. One undo step, "Add audio clip": adds the sample to the pool if the project does not have the file yet, a playlist track at the end if `track` is absent, and an audio clip at tick `start` that is as long as the file lasts at the current tempo. With `mixerTrack` absent the clip plays into the mixer track of the audio clip of the same sample that was made last (the one with the highest id), wherever that clip plays now, so a file dropped three times shares one track; only when no clip plays the sample is a mixer track made, named after the file (the master when the mixer is full). `created` holds, in this order: the sample (the existing one if the file was in the pool), the playlist track if one was made, the mixer track if one was made, and last the clip. A clip that joins an existing mixer track therefore reports no mixer track id: the clip is always the last id, and its `mixerTrack` is in the patch. Rejects an unreadable file, and like `add_channel_from_file` when another project was opened meanwhile. |
| `add_audio_clip_from_sample` | `sample: SampleId`, `track?: PlaylistTrackId`, `start: number`, `mixerTrack?: TrackId` | `DispatchResult`. The same for a sample already in the pool, with the same rule for an absent `mixerTrack`. `created` holds the playlist track if one was made, the mixer track if one was made, and last the clip. Rejects a sample whose audio is not loaded. |
| `export_audio` | `options: ExportOptions` | Returns at once. Progress arrives as events. Writes one file, or with `stems` one file for each stem. Adds the format's extension to a path with none and rejects any other extension. `autoTail` ends the file when the sound has rung out, at most `tailSecs` after the end. Rejects what the format cannot be, with a message for each case; see Export below. |
| `export_playlist_tracks` | `options: ExportOptions`, `stems: ExportPlaylistStems` | Like `export_audio`, with one file for each playlist track: the song with only that lane's pattern and audio clips placed, heard at the output through its mixer tracks, buses and the master. Automation plays in every file. Needs song mode, and `options.stems` must be absent. Follows the active arrangement. See `docs/integration/seams/playlist-track-export.md`. |
| `consolidate_playlist_tracks` | `tracks: PlaylistTrackId[]` | Renders each track's unmuted pattern and audio clips to one audio clip on that track, played straight to the output, and mutes the sources: one undo batch for any number of tracks. Resolves to a `DispatchResult` whose `created` holds a sample and then its clip for each track, in playlist order. Changes nothing when a track has nothing to render, a render fails, or the project was edited meanwhile. |
| `export_cancel` | | Stops the export that is running, if one is. It ends with a last `export:progress` event that has `cancelled` set, and leaves no file. |
| `midi_preview` | `path: string`, `options: MidiImportOptions` | `MidiImportPreview`: names, counts and adjustment sentences, with a token for the exact prepared plan. Only the latest review is retained; another document or newer preview invalidates it. Does not edit the project. |
| `import_midi` | `token: number` | `DispatchResult`. Appends the reviewed channels, patterns, playlist and optional tempo automation as one checked "Import MIDI" undo step. Allocates against the current document, so intervening edits are preserved. Rejects expired reviews and invalid batches without changing the project or history. Factory drum samples use the normal sample loader. |
| `midi_discard` | `token: number` | Releases a cancelled review, if still current. Does not edit the document. |
| `export_midi` | `path: string`, `options: MidiExportOptions` | `string`, the saved path. Writes pattern or song MIDI atomically from a checked snapshot; adds `.mid` if absent and accepts `.mid`/`.midi`. Supports formats 0/1, 480/960 PPQ, running status and swing. No audio is rendered. |

Events the shell emits to every window:

| Event | Payload | When |
|---|---|---|
| `project:patch` | `ProjectPatch` | After every edit, undo and redo, from any window. |
| `project:loaded` | `DocumentSnapshot` | After new and open. A save that had to rename a clashing sample sends an ordinary `project:patch` with the samples instead and keeps the undo history. Only when the new name is one a sample in the undo history already has is the open document replaced by the one just written, with the same ids and a fresh history, and announced here. A "Save as" into another folder that was overtaken by an edit is finished all the same: the project moves to the new file, renamed samples are pointed at their new names, the edits are kept, and the patch says the project still has unsaved changes. A sample those edits brought in from the old project folder is pointed at its file where it is. The save is refused only when both happen at once: an edit during the save, and a new name that the history already has. |
| `transport:state` | `TransportState` | When the transport changes. |
| `engine:status` | `EngineStatus` | When the audio device changes or fails. |
| `export:progress` | `ExportProgress` | While exporting. `path` is the path exactly as the UI sent it, also when stems are written under other names. `fraction` runs over the whole export, every pass of a stem export included. The last event, with `done` set, has in `droppedClips` how many audio clips are not in the file because more than 128 would have played at once; it is 0 on every event before. Only the last event has one of these: `error`, why the export failed; `cancelled: true`, after `export_cancel`; or `files`, what was written, as `{ path, track, frames, bytes }` with the mix first (`track: null`) and the stems after it in mixer order. |
| `midi:note` | `MidiHardwareNote` | For every note of a MIDI input that the audition destination accepted: `{ channel, key, velocity }`, with `velocity` 0 for a release. The UI keeps these in its played-note log, which "Dump played notes" places on the shared snap. A note the engine refuses is not reported. |
| `project:warnings` | `string[]` | After a project loads with problems, such as a missing sample file, and after `samples_reload`. |

Runtime types that grew with effects, all in `windfall-ipc`:

- `RealtimeFrame.voices` counts instrument voices as well as sampler voices. Audio clips are not voices: `RealtimeFrame.audioClips` counts the ones playing, at most 128, without those that are fading out after a stop, a seek or an edit.
- `RealtimeFrame.droppedClips` counts the audio clips that should be sounding right now and are silent because 128 were playing when each was to start. It is 0 while the song is not playing, goes back to 0 as those clips' ends pass, and counts no more than 1,024.
- `RealtimeFrame.gainReductions: { effect: EffectId, db: number }[]` has one entry per compressor and limiter of the project, in mixer order and then chain order. `db` is the deepest reduction since the last frame, 0 for none.
- `EngineStatus.latencyFrames` is the latency the project's effects and instruments add, in frames at `sampleRate`. It changes with the project, so ask for the status again after an edit to an effect. It is the figure of the project the engine was last handed, at once: an instrument of the project before it that is still fading out does not count.
- `ExportOptions.autoTail` is described above. It defaults to off.
- `RealtimeFrame.automated: { automation: AutomationId, value: number }[]` lists the automations that have their target in hand right now, in the order of the project's automations, with the value from 0 to 1 each is giving it: while a clip of it plays, while the target holds what the last clip left, and after the song has stopped for as long as it rings out with the target held. So it can have entries while `playing` is false. Where several automations move one target, the one whose clip decides is listed. It is empty in pattern mode and once a stopped song has let go of its targets, and holds at most 128 entries. A control whose target is listed is being moved by the song and shows this value instead of the stored one.

The UI store applies a patch only when `patch.revision` is exactly one more than the revision it holds. A patch it has already applied is ignored. On a gap it calls `document_snapshot` and replaces its copy.

Replies and events travel separately, so a reply can arrive after an event that is newer. Events are the truth:

- Transport and engine status: the reply of a command is used only when no event of that kind has arrived since the command was sent. A play that the engine ends at once therefore cannot leave Play lit.
- Project: replies carry patches, and patches are ordered by revision, so a late reply is ignored like any patch already applied. A snapshot that turns out older than a patch seen while it was on its way is fetched again.
- Saving: the UI records only the path a save returned. Whether the project is clean comes from the patch the shell emits after every save, because the shell knows about an edit made while the file was being written. A project without a path (never saved, or an opened backup) is saved through Save As.

The UI also has file dialogs on the `Backend`, which are not shell commands: `pickProjectToOpen`, `pickProjectSavePath`, `pickExportPath`, `pickFolder` and `pickAudioFile` (wav, wave, aif, aiff, aifc, flac, mp3, ogg, oga).

### Export

An export renders a copy of the project straight into its encoders: `render_streaming` or `render_stems` on one side, a `windfall_codec::Encoder` for each file on the other. Nothing but the block in hand is in memory.

| `format` | Extension | `bitDepth` | Sample rates | Settings |
|---|---|---|---|---|
| `wav` | `.wav` | `int16`, `int24`, `float32` | 8,000 to 384,000 Hz | none |
| `flac` | `.flac` | `int16`, `int24` | 8,000 to 384,000 Hz | `flacLevel`, 0 to 8, 5 if absent |
| `ogg` | `.ogg` | ignored | 8,000 to 192,000 Hz | `oggQuality`, -1 to 10, 6 if absent |
| `mp3` | `.mp3` | ignored | 8,000, 11,025, 12,000, 16,000, 22,050, 24,000, 32,000, 44,100, 48,000 Hz | `mp3: { rate, channels }`, 192 kbit/s joint stereo if absent |

MP3 `rate` is `{ "mode": "cbr", "bitrate": 192 }` (128, 192, 256 or 320 kbit/s) or `{ "mode": "vbr", "quality": 2 }` (V0 best through V9 smallest). `channels` is `mono`, `stereo` or `jointStereo`; mono averages the engine's stereo pair. CBR 128 requires at least 16,000 Hz; higher CBR rates require 32,000, 44,100 or 48,000 Hz. Lower rates accept VBR. Unsupported combinations fail before rendering; the dialog should choose a supported render rate, as the encoder never silently resamples.

Vorbis uses `vorbis_rs` 0.5.6 (BSD-3-Clause), whose vendored libogg and aoTuV/Lancer libvorbis sources build through `cc` without system codec packages. MP3 uses `mp3lame-encoder` 0.2.5 and `mp3lame-sys` 0.1.11 (both LGPL-3.0), with vendored LAME 3.100 (LGPL-2.0-or-later). Those licenses are compatible with GPL-3.0. LAME builds directly with `cc` on Windows/MSVC and uses its bundled configure scripts through `autotools` on Unix, which needs a C compiler, make and a POSIX shell. Neither format is feature gated. The Ogg stream serial is fixed; canonical input blocks keep Vorbis bytes independent of the renderer's block size. The final MP3 Xing/Info and LAME header is patched with frame counts, encoder delay and padding for gapless decoding. Determinism is verified within one build; floating point codec implementations on different architectures need not produce identical lossy bytes.

- Integer files are dithered with triangular noise from a fixed seed, the same for WAV and FLAC, so a FLAC file holds exactly the numbers a WAV file of its bit depth holds and the same export gives the same bytes every time. The FLAC encoder is Windfall's own (`windfall_codec::FlacWriter`): fixed and fitted predictors, Rice coding, the MD5 sum of the audio in the header.
- A setting of a format that is not the one chosen is not looked at. What the chosen format cannot be is refused before anything is rendered, each with its own message: a file name that ends in another format's extension, 32-bit float FLAC, a level out of range, and an export longer than its file can hold (4 GB for WAV, which is 186 minutes of 32-bit float stereo at 48 kHz; a day for the others).
- Every encoder writes under a temporary name beside its destination (`.<name>.<pid>-e<serial>.tmp`). The shell finalizes all files under staging names before placing any destination. Existing destinations move to backups during placement; a failed placement removes new files and restores those backups. Failure or cancellation before placement leaves existing destinations untouched and removes temporary and staging files. A group of renames is not crash atomic: interruption of the process during placement can leave staging or backup files for recovery.
- With `stems: { mode, tracks?, includeMix, numbered, folder }` the export writes the streams of `render_stems`, all in the one format. For the path `Song.flac` the files are `Song - Mix.flac`, `Song - Bass.flac` (`Song - 03 Bass.flac` with `numbered`) and so on, in the folder `Song` beside it with `folder`, and where `Song.flac` would have been without. `Song.flac` itself is not written. A folder the export made is removed again if the export does not go through. What cannot be a stem is refused up front: the master, a track the project does not have, and no track at all without the mix.
- One export runs at a time. `export_cancel` takes effect at the next block rendered.

## UI rules

Desktop CLAP bindings are persisted as plain project data; the engine receives prepared native audio halves through an optional factory in `SamplePool`. Windows owns instance/state/editor lifecycle on a dedicated event thread and scans plugins in child processes. Installed audio plans select playback ownership atomically; offline and speculative instances cannot supply project state or editor changes. See [desktop plugin integration](plugins/desktop-integration.md) for the IPC shapes, lifecycle, PDC, missing-plugin behavior and platform limits.

- Every user action is a named entry in the action registry: id, title, default shortcut, run function. Menus, the command palette, context menus and the keymap all read the registry. Nothing binds a key or builds a menu item any other way.
- Shortcuts have a scope. An action with `scope: "pianoRoll"` gets its keys only while the piano roll has the keyboard, so Delete, Ctrl+D, the arrows and the tool letters can mean something else in every panel. A panel marks its root element with `useShortcutScope("pianoRoll")`. A key goes to the innermost scope around the focused element, then outward, then to the global shortcuts, and runs the first action bound to it that is enabled. A disabled action does not hold on to its key, and a key nothing can use is left alone. While the focus is on nothing a panel owns (the page after a click on a canvas, a transport button, a menu), the keys go to the panel that was clicked or focused last, and before that to the center tab in view. A dialog gets only the global shortcuts. Every label (menus, palette, tooltips, context menus) shows the key an action has in its own scope.
- A part of a panel can be a scope of its own inside the panel's: the channel settings (`rackInspector`), the mixer's effects (`effectInspector`), the settings of the selected audio clips above the timeline (`clipInspector`) and one effect, as its slot on a strip or the header of its panel (`effect`). An inner scope may keep Edit commands from the scopes around it: `useShortcutScope("rackInspector", { keeps: INSPECTOR_KEEPS })`. A key that would run a kept command further out does nothing instead, so Delete and Ctrl+D pressed in a channel's settings never delete or duplicate the channel, and Edit > Delete is off there. Every other key goes on outward as usual.
- Delete and Backspace delete things and nothing else. A value control does not take them: a focused knob or fader goes back to its default by double-click, by Ctrl+click or by "Reset to default" in its menu, and Delete pressed on it goes to the scopes around it like any other key. A command that selects something in another panel moves the keyboard there with it ("Create automation clip" and "Show automation" focus the timeline), so the next key acts on what is selected.
- The text entry of a value control sets a value on Enter and on Tab. Escape cancels, and so does leaving the field any other way. A key with Ctrl, Alt or Cmd held never starts an entry; it is a shortcut.
- Keymap presets are data. An action's `defaultShortcut` is its key in the Windfall preset; a panel hands the keys its actions have in another preset to `registry.register(actions, { presets: { fl: { ... } } })`. A preset's key applies inside the action's own scope.
- One rule decides what the focused control keeps, in `shortcutAllowed` in `lib/actions/keymap.ts`. Text entry (inputs, text areas, editable text, combo boxes, the inline value entry of a knob) keeps every plain key and the text-editing shortcuts. An open menu, list or dialog keeps every plain key. Anywhere else Space is the transport's: it plays and stops even with a button, slider, step or tab focused, and the keymap takes it before the control sees it. Enter presses the focused control, so a shortcut on plain Enter does not fire on one. For every other key the focused control goes first: a control that uses a key calls `preventDefault`, and the keymap then leaves it alone.
- `enabled` and `checked` get an `AppState`: the document, the transport and the UI store's selection (`selectedChannel`, `selectedTrack`, `centerTab`, `activeScope`). State in a panel's own store may be read there too, as long as the panel tells the registry when it changes: `invalidateActionsOn(usePanelStore, (state) => [state.selection, state.tool])` next to `registry.register`, or `registry.invalidate()` by hand.
- The menu bar is the list in `lib/actions/menus.ts`. Every id in it must be registered once the app has started; a test checks that through `registerAllActions`. Edit > Cut, Copy, Paste, Duplicate, Delete and Select all run the action of the active panel that declares that `editCommand`.
- In production builds `lib/webview-guard.ts` turns off the webview's own right-click menu (except in text fields), its browser keys (reload, find, print, zoom, back) and Ctrl+wheel page zoom. Development keeps them.
- A right-click opens the app's own menu everywhere: on every thing with actions of its own, on the empty space of every panel, and on the title bar, the transport bar, the editor tabs and the status bar. Menus are made with `ContextActions` from registry action ids, plus inline entries for the thing clicked. They nest, and the innermost one opens. A right-click that a control uses for itself (erasing a step, soloing with a mute lamp, deleting in the Draw tool) opens none. A text field keeps the menu every text field has (`lib/text-field-menu.ts`).
- Every value control (knob, fader, pan control, number field, and so every `ParamControl`) has the same menu: reset to default, type in a value, copy and paste. The kit stays free of app code: it hands each control to `ValueControlSlot`, and `components/value-context-menu.tsx` fills that slot for the whole window with one shared menu. A control bound to something in the project adds entries about that thing, put first, in one place per binding: `useParamBinding`'s `contextItems` for the settings of effects and instruments, `trackValueItems` in `features/mixer/menus.ts` for a strip's fader and pan, and `channelValueItems` in `features/channel-rack/menus.ts` for a channel's knobs. That is where "Create automation clip" goes. A copied value is a number with the kind of unit it is in (`unitKind`: the kit's units and every `ParamUnit`), and Paste is off, with a reason, on a control of another kind: a level never lands on a pan.
- Selecting something never moves an editor's grid. A strip that shows the settings of the selection (the audio clip settings above the timeline) keeps its place and its height whether anything is selected or not. The grids also measure a drag from where the grid was when the button went down (`createPointerFrame` in `lib/canvas`), so a layout change under a still pointer is never taken for movement.
- What is drawn over the edge of a thing can be taken where it is drawn: the points and bend handles of an automation clip are hit-tested before clip bodies and before the empty grid, out to the edge of their dots.
- How much of its range an automation's clips show (`features/playlist/automation/view-store.ts`) is view state: stored values stay 0 to 1, undo leaves it alone, and it is dropped when another project is opened. A tempo curve defaults to a window about the project's tempo and its points; everything else to the whole range.
- Toasts come up at the bottom right above the status bar. An element marked `data-toast-clear` that reaches that corner, as the mixer's docked effects do, is kept clear: toasts move to its left (`features/layout/toast-place.ts`).
- Any deletion that takes automation with it says so first. `features/automation/owned.ts` works out what goes with a channel, a mixer track, an effect or a send. Removing or replacing an effect and removing a send ask only when there is some.
- The modifier keys mean the same in the piano roll and the playlist (`lib/edit-modifiers.ts`): Ctrl+drag on a note or clip copies, decided at the drop; Ctrl+drag on empty grid selects with a box; Shift adds to the selection and never copies or changes the snap; Alt lets go of the snap. A keymap preset changes keys only.
- Project state comes from the store, which mirrors the `Document`. Components never keep their own copy of project data. A control being dragged shows its local value and dispatches with a gesture id.
- Ids start over in every project, so anything kept by id outside the project (held peaks, a lane's scroll position, folded effects, a selection) is dropped when another project takes the place of the open one. `onProjectReplaced(listener)` in `lib/store` is the one signal for that. It fires on `project:loaded`, after the stores hold the new project. `useProjectGeneration()` counts the projects loaded, as a `key` for a panel that should start over.
- Realtime values (playhead, meters) do not go through React state. Components subscribe to the realtime feed and draw to a canvas or set a style inside `requestAnimationFrame`.
- Canvas drawing reads colors from the same CSS variables the shadcn theme defines, so both themes work.
- Audio controls come from `components/audio`. Features do not build their own knobs.
- The editor of a built-in instrument or effect is made of `features/params`: `ParamControl` picks the control for a setting from its descriptor, with its name, unit, status-bar line and default, and `useParamBinding` sends each drag as one gesture through `setInstrumentParam` or `setEffectParam`. `GenericParamEditor` lays out any descriptor with no code of its own. Its README is the reference.
- An action that is for one kind of thing says so while it is disabled: `whyDisabled` returns a few words ("Samplers only"), which menus, context menus and the palette show where the shortcut would be.
- `EngineStatus.latencyFrames` follows the project, so the engine store asks for the status again shortly after a patch that touches the mixer or the channels.

## Legal rules that bind every change

- No decompiling or disassembling FL Studio or its plugins.
- No Image-Line samples, presets, artwork or plugin names in the repo.
- FL names appear only as plain references, such as "FL equivalent: X" in docs.
- Check every new dependency's license against GPL-3.0 before adding it. MIT, BSD, Apache-2.0, ISC, Zlib, MPL-2.0 and GPL-3.0-compatible licenses are fine.

## Audio input recording

Native capture, its callback queue and owning worker live in windfall-engine's recording module. The shell's session/recording module owns the take and streams through the codec's WAV writer, then imports it through the ordinary audio-clip command pipeline. The recording_inputs, recording_state, recording_start, recording_stop and recording_cancel commands serve the transport recording dialog. See [RECORDING.md](RECORDING.md) for timing, rate, ownership and cleanup semantics.

## MIDI hardware

`windfall-engine::midi_hardware` owns native MIDI handles and output sends on one
control worker. A preallocated callback queue and atomic panic epoch feed the
existing audio processor's live-note path. The session guards runtime destinations
with document generation/revision and serializes device changes under recording
exclusion. The settings UI uses the `midi_hardware_*` IPC commands; Standard MIDI
File import/export in `windfall-midi` remains separate. See
[MIDI-HARDWARE.md](MIDI-HARDWARE.md) for supported events, ownership, tests and
physical-hardware verification limits.
