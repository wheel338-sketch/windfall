# Windfall Script v1 and the offline assistant

Open **Help → Offline assistant** or **Help → Script editor**. The assistant
answers from the bundled `ASSISTANT-HELP.txt`. It recognizes `create pattern
NAME`, `add channel NAME`, `set tempo BPM`, `route channel ID to track ID`,
`route track ID to track ID`, and
`mute channel ID` / `unmute channel ID`. Channel and track IDs are shown in the
panel. Creating a channel uses Windfall's subtractive synth.

Every proposed edit shows its exact command payload. Nothing changes until
**Apply batch** succeeds. A successful batch makes one undo entry; **Undo
batch** is available while that result is still the latest document revision.
An unrelated edit or project replacement invalidates approval. Invalid,
oversized, ineffective or recording-excluded proposals report an error and
leave the project/history/ID counter untouched. Dismissing a proposal does
not apply it. Cancelling a response ignores any late result.

## Language and IO policy

Windfall Script is a small deterministic Rust interpreter bundled with the
application. No runtime installation, native dependency, provider SDK or
network access is required. It does **not** execute Python and does **not**
claim source compatibility with another product's piano, device or modular
scripts. Porting those scripts requires rewriting them in this language.

One statement occupies one line. Whitespace separates words. `#` starts a
comment. Expressions use postfix notation: `key 12 +` adds twelve to key;
`velocity 0.5 *` multiplies velocity by one half; `key 12 + 0 127 clamp`
clamps the sum. Operators are `+`, `-`, `*`, `/`, `min`, `max`, `clamp`.
`a b -` means a minus b; `value minimum maximum clamp` applies the bounds.
There are finite numeric literals, declared parameter names and the eight
register names below. There are no arbitrary variables, calls, strings,
branches, recursion, imports or unbounded loops.

There is no filesystem, network, process, clock, random, credential, native
plugin-state or mutable project interface. The interpreter accepts plain
note/input values and produces plain output. Its adapters alone construct
checked commands. The only external script output is bounded CC feedback
from an installed controller binding with explicit feedback opt-in, through
the already configured native MIDI output. Merely editing, previewing or
saving source never sends messages.

Statements:

```text
param NAME MIN MAX DEFAULT
set FIELD EXPRESSION
selected
  ... statements ...
end
repeat INTEGER_COUNT
  ... statements ...
end
emit
tempo EXPRESSION
map EXPRESSION
feedback CC_NUMBER EXPRESSION
```

Parameter declarations come first. Names contain ASCII letters/underscores,
up to 32 bytes; names must be unique and cannot shadow registers. The parameter
form reads these declarations; supplied values must be finite and within the
declared range. An empty parameter vector uses all defaults; a nonempty vector
must supply every parameter in declaration order.

Writable registers are `start` and `length` (integer ticks after rounding),
`key` (0..127 after rounding), `velocity` (0..1) and `pan` (-1..1).
Read-only registers are `index` (zero-based loop position), `value`
(normalized controller input, 0..1) and `number` (CC number or note key; zero
for pitch wheel/channel pressure). `selected` loads each selected note's five
fields, in the order supplied by the editor. `repeat` loads a fresh default
note each iteration. Loops cannot nest. Before loops, defaults are tick 0,
one step (240 ticks), key 60, velocity 0.8, pan 0. Parameters persist across
iterations; emitted notes do not change the source selection.

`emit` appends a note. Fractional start/length/key are rounded; final length
must be at least one tick and the note's end must be within the project's
maximum pattern range. Division by zero, non-finite arithmetic, inverted
clamp bounds and invalid emitted musical values fail with a one-based source
line. Admission/input errors use line zero. Failed runs return no partial
notes or messages.

## Piano scripts

Native piano execution runs on a blocking control worker, never on audio.
Open a piano-roll lane, select notes, read parameter controls and choose
**Preview notes**. Review the output and commands before applying. **Replace
selected notes** deletes only the captured IDs; disabling it appends output.
Generated notes receive new IDs and default per-note expression. Undo restores
the original notes, IDs, expression and curves. Unselected notes are unchanged.
The editor reports this expression difference before applying.

Transpose selected notes with a parameter:

```text
param transpose -24 24 12
selected
set key key transpose + 0 127 clamp
emit
end
```

Generate a four-note ascending sequence without a selection:

```text
param root 0 120 60
repeat 4
set start index 240 *
set length 180
set key root index +
emit
end
```

Only note output is admitted for piano scripts. `tempo`, `map` or `feedback`
output rejects the entire piano result. Preview captures generation/revision,
pattern/channel and selected IDs; apply checks the project again. Cancellation,
a new run, a changed project or an expired deadline invalidates completion.
Browser mode can compile parameter declarations, save source revisions and
review commands through Rust WASM, but explicitly reports that native piano
execution and hardware controllers are unavailable.

## Controller scripts and existing profiles

A saved controller binding declares a `ControlSource::Midi`, exact script
revision, parameters, optional checked `LinkTarget::Param` / `Slot` / `Action`, and
optional feedback channel (1..16). Device/channel wildcards and CC, note,
pitch wheel and pressure sources follow `CONTROLLER-LINKS.md`. Normalized input
is supplied by the existing controller hub. A script binding takes precedence
over ordinary links for the same incoming source. Other sources continue
through the existing profiles, learning, pickup and transport action handling.
Scripts can select checked transport/button targets from the existing action
registry. Their mapped output uses its rising-edge runtime; it cannot invoke
arbitrary UI actions.

`map EXPRESSION` sets the binding's target using its existing automation
range and checked target command; the last map output wins. It must be 0..1.
`tempo EXPRESSION` emits a checked tempo edit within project bounds (10..522).
`feedback CC_NUMBER EXPRESSION` emits a CC value 0..127, rounded to an integer.
Feedback remains disabled unless the binding explicitly opts in. A controller
script cannot emit piano notes.

Invert a controller and echo its position to the controller's CC ring:

```text
map 1 value -
feedback 7 value 127 *
```

Map normalized input to tempo:

```text
param minimum 20 200 60
param span 0 300 100
tempo minimum value span * +
```

The installation form chooses an exact device name or any enabled input, an
input channel or all channels, and CC, note/pad, 14-bit wheel or pressure.
Choose a positional, project-parameter or checked action target, takeover mode,
smoothing (0..5000 ms), encoder/toggle mode and feedback channel. Parameter
choices use the existing checked automation capabilities, including hosted
parameters and rack control inlets. Review the exact source/target/map/feedback
in **Review save and assign**. **Edit controller assignment** restores its saved
source, parameter values and mapping; saving creates a new immutable revision.
Removal is reviewed and undoable without deleting source revisions.

The controller pump interprets outside document/hub locks, then feeds map/tempo
outputs through the ordinary link runtime. Pickup compares the scripted output
to the target's normalized value. Smoothing continues on idle pumps, controls
within 600 ms share a gesture, and armed song/pattern automation capture records
the actual stored movements using the ordinary pass gesture and audible touch
path. Relative modes decode the original MIDI data byte. Old saved bindings
default to immediate takeover with no smoothing. Pickup holds, interpreter
diagnostics and message drops are visible in the script editor and controller
settings. Each pump's script and ordinary link edits form a generation/revision
guarded batch. Errors and event overflow refuse the batch and clear pending
script glides. Audio recording exclusion still prevents document edits and
feedback; automation-record arming is supported.
There is no output while source is merely being authored. Feedback preflight
rejects a disconnected output before applying an internal batch. A disconnect,
full queue or stale document after internal commit cannot undo physical MIDI:
the native worker reports drops/errors, and the session warns if the command
batch succeeded but feedback enqueue failed. Hardware epochs discard old
queued packets when the output/project context changes. Unrelated profile
transport/clock actions keep their existing separate semantics.

## Compiled channel note transforms and rack hookup

Note transforms accept parameters, straight-line `set` expressions, optional
`copies COUNT KEY_STEP TICK_STEP` and exactly one final `emit`. Copies has a
constant count 1..16, a signed semitone step and nonnegative tick step. Each
copy retains the transformed duration and original expression source; the host
creates independent sounding instances. Every key/end is checked before any
output is admitted. They reject loops, controller actions and feedback. Choose
**Note transform**, select a channel and **Review save and assign**:

```text
param transpose -24 24 12
set key key transpose + 0 127 clamp
set velocity velocity 0.5 *
emit
```

`Program::note_transform(parameters)` builds an owned `NoteTransform` with
fixed instruction/register arrays. `NoteTransform::evaluate(ScriptNote)` remains
the first-output API; `evaluate_notes` returns a fixed array of up to 16 outputs.
Both use fixed stack storage and return a small `TransformError`
without allocation, frees, locks, clock queries or IO. Prepare it before
publishing an engine plan. Do not store `Program` or interpreter output vectors
in a callback processor.

Current engine hosting is the channel side table
`ProjectSettings.scripts.channels`. `engine/src/script_notes.rs::prepare`
prepares it on the control side; `engine/src/plan.rs::compile_pattern`
evaluates before channel timing/swing and bakes transformed tick/key/length/
velocity/pan into immutable sequencer events. Live sequencing and export
consume that same table. Original note IDs/source expression remain attached.
Checked project loading/edits validate all stored notes against assigned
transforms; invalid runtime results reject the edit transaction. Unchecked
engine fixtures preserve the input event on evaluator failure.

The modular-rack worker can reuse the public `Program`, `NoteTransform`,
`ScriptNote`, `TransformError`, `ScriptReference` and `ChannelScript` types.
Its node should retain an exact script revision, compile off audio, own fixed
bytecode in immutable runtime storage and map its note event to/from ScriptNote
while preserving note identity/expression. It must retire runtime owners off
audio and define pairing/ordering when transforming note-on and note-off.
This stream does not add a modular-rack node or alter module automation. Channel
scripts feed the existing sequenced channel hook before rack/instrument/MIDI-out
delivery. The immutable pattern extent grows to fit transformed ends; clip
playback grows by its repeated-pattern extension. Scripted notes admitted in a
short clip window keep their own release tick and the render/song extent includes
their tails. The checked document refuses expansion beyond the song bound.
Authored placements remain saved; reopening derives the same playback extents.
Hardware audition, automation targets for script parameters and live parameter
re-evaluation are still SOFTWARE gaps; the UI continues to report this limitation.

## Audio-selection scripts

Open a playlist clip's **Audio editor**. Choose a frame selection, load an original
example or write source, read its parameters and choose **Run script**. The worker
returns a waveform of the processed selection without writing a file or editing
the project. **Audition script result** plays that cached selection. **Apply script
result** attaches exactly the reviewed audio through the guarded derived-WAV edit
path as one undo step. Cancel invalidates the native request and cached preview;
it is rechecked at publication. Source/project changes refuse stale work. Save and
fresh reopen retain the resulting audio through ordinary project sample storage.

Audio uses the same postfix compiler and arithmetic evaluator, with these
selection-only statements (no notes, controller actions, feedback or loops):

```text
param first 0 1 0
param last 0 1 1
gain first last first - value * +
```

`value` is normalized frame position 0..1, `index` is the zero-based frame number,
and parameters use the existing checked declaration/order rules. Every statement
is a pass over a private selection copy. `reverse` reverses frames with stereo
pairing intact. `normalize AMPLITUDE` computes linked peak gain, refuses silence,
and requires a target 0..1. `gain EXPRESSION` applies linked frame gain and refuses
non-finite/full-scale overflow. Other shipped examples are `reverse`, `normalize
peak` with a parameter, `gain value` (linear fade in) and `gain 1 value - 1 value - *`
(squared fade out). Audio outside the selected range is unchanged.

Parsing, rendering and derived-file work run on blocking control workers. There
is one admitted script worker shared with piano scripts, a cooperative five-second
execution deadline and at most 100 million counted instructions/sample visits.
Source/bytecode/stack/parameter bounds remain unchanged. Input, selection scratch
and output each obey the editor's two-minute/64-MiB per-buffer cap; one preview
result is retained. Cancellation is checked throughout every pass, including
validation, peak scans and reverse. Apply uses the cache and never reruns source.
The deadline bounds interpreter computation, not OS file IO or engine preparation.
Browser mode explicitly refuses native audio processing and audition.

## Bounds, persistence and provider decisions

Limits are 16,384 source bytes per script, 512 compiled instructions, 32 stack
values, 24 registers including at most 16 parameters, 4,096 input/output notes,
constant repeat counts 1..4096 and no nested loops. Piano defaults to 100,000
instruction steps. Controller events allow 5,000 steps, zero notes and 16
tempo/feedback messages; one pump captures at most 32 script events, 128 checked
commands and 128 feedback packets. Excess input events increment the existing
controller drop count and report a budget error. MIDI feedback uses a bounded,
nonblocking native request queue. The general runner refuses budgets above
1,000,000 steps, 4,096 notes or 128 messages.

Native piano has one admitted worker; a second request gets an explicit busy
error. Cancellation is checked per instruction. Piano/pump deadlines are
cooperative 100 ms; native code checks completion as well. This is bounded
language work, not OS preemption of snapshot cloning or engine preparation.
There is no user process that can die or hold an uninterruptible host call.
All interpreter allocations are on control workers; only fixed transforms
are eligible for realtime use.

Library limits: 64 revisions, 128 channel assignments, 64 controller
assignments, 262,144 aggregate source bytes. Script IDs are 1..64 ASCII
alphanumeric/`-`/`_` bytes and revisions are positive integers. Editing an
existing saved revision in place is rejected: save a new revision. A binding
references an exact `{id, revision}`, language version and parameter vector.
Unknown language versions and missing/wrong-kind revisions fail loading.
Deleting a target leaves its assignment suspended so undo can restore it;
new assignments must resolve. Legacy projects omit/default the entire script
library. Saving and reopening source/assignments uses normal project storage.

Proposals allow 1..128 commands and 524,288 serialized bytes, labels up to
128 bytes and explanations up to 4,096 bytes. The capability allowlist lives
in `project/src/proposal.rs`: it excludes recursive batches, filesystem,
project replacement and opaque plugin state. Checked native parameter
capabilities and recording locks still apply. Providers and scripts do not
get mutable document handles.

## Optional bring-your-own-key provider

Offline help stays available. The native panel also has **Bring your own key
connection**: enter a chat-completions base URL, model name and API key, enable
the connection, then save. It remains OFF without a key. A blank key field
keeps a previously saved key; **Remove key and turn OFF** deletes it. Status
never returns the stored key. Saving, loading a project and opening the panel
do not send requests. Only **Ask BYOK assistant** sends one. Browser mode
explicitly refuses credentials/remote requests instead of saving keys there.

The native adapter implements the same `AssistantProvider` trait using
Tauri's existing HTTP client (`reqwest`), without a model SDK. The endpoint
must use HTTPS; literal 127.0.0.1/::1 HTTP is allowed for local services and
loopback fixtures. `/chat/completions` is appended to the base URL unless it
is already present. URL credentials, queries and fragments are rejected.
Redirects and automatic proxies are disabled so a bearer key cannot follow
a response to another endpoint.

This version uses the authorized fallback credential file
`assistant-connection.json` in the native per-user app configuration directory,
outside projects/archives. It is atomically replaced, private 0600 on Unix,
and inherits the app-config directory's Windows ACL. Its key is plaintext:
there is no OS credential-store adapter installed. The private struct has no
Debug implementation; request headers are marked sensitive. Request/response
bodies and transport errors are never logged; displayed transport/parser
errors are generic. Replies containing the stored key are discarded before
they can become answers or project commands.

The request sends `model`, `stream: false`, `max_tokens: 4096`, and system/user
messages. The system message contains the command schema and at most 128 IDs
each for channels, patterns and mixer tracks. The user message is your prompt.
It does not send full project data, samples, paths, names, notes, plugin state
or the key in message content. The panel displays this data policy before Ask.

The endpoint returns a chat completion whose `choices[0].message.content`
contains one JSON object, without code fences:

```json
{
  "answer": "Proposed a tempo change.",
  "label": "Tempo",
  "explanation": "Review the tempo change before applying it.",
  "commands": [{"type": "updateSettings", "patch": {"tempoBpm": 127}}]
}
```

An empty `commands` array is an answer/refusal with no proposal. Unknown fields,
missing fields, tool-only replies, malformed JSON and commands outside the
advertised schema are rejected. The current BYOK schema exposes pattern
creation/rename, subtractive channel creation, tempo, channel mute/mixer
routing, mixer name/volume/pan/mute/solo and checked track-output routing.
It deliberately omits opaque plugin actions, script installation, recursive
batches and external IO. The Rust document supplies the approval stamp,
validates the whole batch and checks the stamp again after the reply; the
model cannot approve or apply its own commands.

Native limits: one admitted request, 15s HTTP total timeout (5s connection
timeout), 65,536 request bytes, 600,000 response bytes, 524,288 inner reply bytes,
8,192 answer bytes and the existing proposal limits. Cancel wakes the worker
and drops its HTTP future; the UI also ignores late replies. Client setup and
snapshot/preparation work are control-side and not preempted by that timer.
Model quality, service retention/cost and real endpoint credentials remain the
user's service choice and an external acceptance gate. The worker only wrote
the loopback tests; it made no network request and executed no tests.
