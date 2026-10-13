# Stem separation and remixing

This is a user-installed CPU ONNX Runtime feature. Windfall contains neither
runtime binaries nor trained weights, makes no network request, and never uses
the deterministic reference backend as a production result. Round-four
acceptance includes compilation, deterministic suites and a real UMX-HQ/ORT
synthetic-mix harness at 44.1 and 48 kHz. Lifecycle fixtures alone make no
separation-quality claim; real-music listening remains separate acceptance.

In Settings → Stem separation choose the absolute shared-library path, explicitly
import `model.onnx` with its manifest JSON, then save the selected default pack.
From an audio clip choose **Separate into four stems**. A browser audio item's
context menu offers the same entry after ordinary file import. **Remix a song**
loads a song, detects source tempo with the existing detector, opens a complete
clip range, and offers the four-role review. Tempo candidates are reported with
confidence; no estimate silently changes the project tempo or retimes audio.
Audition/select stems and apply to separate playlist rows and mixer inserts,
aligned to the original source, in one checked undo entry. The original mix can
be muted. File import and stem insertion are separate undo entries. Applied
assets survive undo/redo and project save/reopen through the existing asset path.

Library resolution is: saved Windfall library setting, `WINDFALL_ORT_DYLIB`
(acceptance harness), `ORT_DYLIB_PATH`, `Analysis/Runtime/onnxruntime.dll` (or
`libonnxruntime.so` / `libonnxruntime.dylib`), then the last explicitly imported
pack directory. All are local. Once loaded, a library is pinned for the process;
restart Windfall to change it. A wrong architecture, missing dependent DLL,
unsupported opset/operator, bad tensor shape, absent weights, checksum failure,
or nonfinite inference output fails the job with an actionable capability
message. No copy-through result is published as separation. Capability discovery
checks the library file; loading/operator compatibility is checked by the worker.

## Pack layout and contract

```text
my-licensed-four-stem-pack/
  manifest.json       # IPC camelCase JSON, at most 16 KiB
  model.onnx          # single self-contained ONNX file, at most 256 MiB
  LICENSE.weights    # weight terms, separate from runtime/code license
  EXPORT.md          # source version, export/version, preprocessing/evaluation
  onnxruntime.dll    # optional operator-installed CPU library, never committed
```

External tensor weight files are unsupported: export with embedded tensors.
Windfall imports checksummed bytes into `Analysis/Models`, persists bounded pack
metadata in `Analysis/packs.json`, and rehashes the imported bytes on restart and
before inference. Runtime/default-model settings live in `Analysis/runtime.json`.
The original pack directory is remembered only by explicit import. Neither a
manifest nor pack registration installs executable code. The operator must trust
the shared library they explicitly install.

Use the existing `AnalysisModel` fields: `id`, `version`, positive decimal-string
`revision`, lowercase hex `sha256`, decimal-string `bytes` / `maxBytes`,
`sampleRate`, `inputChannels`, decimal-string `maxInputFrames`, `outputs`, and
`provenance`. Provenance must include origin, sourceRevision, author, licenseSpdx,
licenseReference, adapterId=`windfall-onnx-separation`, adapterVersion=`1`,
device=`cpu`. Exact weight licensing and source revision are the pack author's
responsibility. `outputs` contains exactly drums, bass, vocals, other, with
preserved model channels. Their order need not match the tensor's stem axis;
`onnx.stemRoles` defines that order explicitly.

The additional `onnx` object is:

```json
{
  "opset": 17,
  "inputs": [{"name": "mix", "shape": [1, 2, "frames"]}],
  "outputs": [{"name": "stems", "shape": [1, 4, 2, "frames"]}],
  "sampleRate": 44100,
  "segmentFrames": 262144,
  "overlapFrames": 65536,
  "contextFrames": 4096,
  "layout": "channelsFirst",
  "stemRoles": ["drums", "bass", "vocals", "other"]
}
```

Only waveform Float32 input/output is supported. `channelsFirst` means
`[batch=1,channels,time] → [batch=1,stems=4,channels,time]`;
`channelsLast` means `[1,time,channels] → [1,4,time,channels]`.
`time = segmentFrames + 2 * contextFrames` for both tensors, including context.
Only the time axis may be symbolic, and its symbol must be `frames`; fixed time
dimensions must equal that exact length. Batch, channel and stem dimensions are
fixed. Channels are one or two. Rate is 8–192 kHz. `layout` and `contextFrames`
default to channelsFirst/zero for older manifests. Spectrogram-mask models are
explicitly unsupported; wrap their complete STFT/masking/inverse STFT in a
waveform ONNX export if the runtime supports those operators.

Bounds reject duplicate/missing roles or tensors, control characters, mismatched
rate/channels, excessive rank/dimensions, integer overflow, context larger than
the segment, total tensor time over 1,536,000 frames, and overlap with hop less
than one quarter of the segment. `maxInputFrames` caps selected source frames
(not each model chunk); choose a declared whole-song limit, e.g. `"691200000"`
for one hour at 192 kHz. Model segment length must fit this limit. The source
view is stereo; a mono model explicitly receives a linked stereo downmix.

## Exporting your licensed trained model

Use the trained model's documented waveform preprocessing and inverse transform,
including normalization/scaling, channel conventions, padding, source order and
alignment. Put them inside the exported wrapper so no hidden Python step is
needed at run time. A wrapper must return full-length waveform samples for each
stem, including the declared context, with zero additional output latency.
Record your training/source revision, checkpoint SHA, weight license, export
tool versions, opset, CPU runtime version, chunk/context bounds and known limits.

The following is an export outline to run in your own already-provisioned model
environment, outside Windfall's build. It deliberately does not fetch a model.
`trained_four_stem_waveform_model` must be your actual licensed trained module:

```python
import hashlib, json, pathlib, torch

pack = pathlib.Path("my-licensed-four-stem-pack")
pack.mkdir(exist_ok=True)
rate, channels, segment, overlap, context = 44100, 2, 262144, 65536, 4096
total = segment + 2 * context
model = trained_four_stem_waveform_model.cpu().eval()
# Wrap/reorder here if its source order differs. Return [B,4,C,T].
# Include the model's documented normalization and full inverse transform.
example = torch.zeros(1, channels, total, dtype=torch.float32)
with torch.inference_mode():
    result = model(example)
assert result.shape == (1, 4, channels, total)
torch.onnx.export(model, example, pack / "model.onnx", opset_version=17,
    input_names=["mix"], output_names=["stems"],
    # A fixed chunk export is sufficient and usually easier to validate.
    dynamic_axes=None, external_data=False, dynamo=False)
data = (pack / "model.onnx").read_bytes()
assert len(data) <= 256 * 1024 * 1024
manifest = {
  "id": "your-model-id", "version": "1", "revision": "1",
  "sha256": hashlib.sha256(data).hexdigest(),
  "bytes": str(len(data)), "maxBytes": str(len(data)),
  "sampleRate": rate, "inputChannels": channels, "maxInputFrames": "691200000",
  "outputs": [{"role": r, "channels": channels} for r in
      ["drums", "bass", "vocals", "other"]],
  "provenance": {
    "origin": "your documented training/checkpoint origin",
    "sourceRevision": "your pinned source/checkpoint revision", "author": "your author",
    "licenseSpdx": "your actual weight SPDX expression",
    "licenseReference": "LICENSE.weights", "adapterId": "windfall-onnx-separation",
    "adapterVersion": "1", "device": "cpu"},
  "onnx": {"opset": 17, "inputs": [{"name": "mix", "shape": [1, channels, total]}],
    "outputs": [{"name": "stems", "shape": [1, 4, channels, total]}],
    "sampleRate": rate, "segmentFrames": segment, "overlapFrames": overlap,
    "contextFrames": context, "layout": "channelsFirst",
    "stemRoles": ["drums", "bass", "vocals", "other"]}}
(pack / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
```

Validate the ONNX graph with the export environment's checker. Compare the same
nonzero waveform/impulse/stereo batches against the original trained module at
the pinned runtime version; validate normalization, scale, source order and
latency. A successful zero-input export does not establish source quality.
Some trained architectures/operators cannot be exported to this contract;
choose a supported export rather than changing the manifest to conceal it.

## Streaming, cancellation and publication

The separation worker reads immutable captured PCM files at absolute phase,
uses the existing recording path's shared 32-tap windowed-sinc resampler,
loads context on both sides, runs fixed-size model chunks, trims context,
and performs normalized positive sine-squared overlap-add. Final odd chunks
are padded and clipped to the selected duration. Four intermediate stem files
are sequentially resampled back to the captured rate and exact captured frame
count; double-rounded rate conversions cannot add/remove source frames.
All stem WAVs are independently strict-stream decoded and checksummed.
Long songs add chunk-sized render/decode/inference scratch and disk spools,
instead of a second whole-song rendered/decoded allocation.

Inference and all IO run on the control/worker side, never audio callbacks.
Progress is coalesced work units. Cancellation is checked while decoding,
resampling, emitting blocks, before/after each native inference call, and before
publication/commit. A synchronous ORT run cannot be preempted mid-call by this
adapter; cancellation takes effect before another chunk or any output commit.
The existing exact source/generation/revision/model/request checks reject stale
results. Publication uses verified private staging and per-file atomic
no-clobber writes, followed by one checked prepared document/pool/history batch.
It does not claim a multi-file filesystem transaction; refused application can
retain already-published assets, as documented in ANALYSIS-JOBS.md.

The ordinary playback sample pool continues to own whole-song source/output
assets. Thus total app playback RAM grows with imported song assets even though
separation processing scratch does not grow with song length. The native job
permits at most one-hour views / 1.5 GiB source assets, reserves 768 MiB for chunk
and runtime scratch, and sets native manager admission to 3 GiB memory / 24 GiB disk.
These are cooperative reservations, not an ORT allocator sandbox or measured
RSS guarantee. Scratch includes the runtime's own workspace and model allocations;
imported model bytes are charged separately. The native UMX-HQ 1.0.1 harness
initially measured a process peak of about 852 MiB (later runs reached about 918 MiB),
including the harness, codecs and
playback pool. That exceeded the former aggregate reservation of about 672 MiB,
so the scratch allowance was raised from 512 to 768 MiB. Other packs require
their own memory and quality measurement; the fixed allowance does not establish
a universal bound on arbitrary runtime/model code. See the round-four acceptance
record for exact measurements and provenance.
Large spectral-stretched sources must first be rendered to an audio asset;
ordinary tape/pitch/reverse/pan/fades and tempo-map clip windows stream directly.

## Integrator verification and real-model harness

Ordinary, non-ignored tests:

- `windfall-analysis/tests/stem_separation.rs`: odd-length/rate identity
  reconstruction, absolute-phase rate conversion, sinc round trips, reference
  stem sum, context/order/hostile manifests, cancellation and file capture,
  explicit missing-runtime failure.
- `windfall-engine/src/audio_edit/streamed_view.rs`: exact editor-versus-streamed
  PCM with odd block partitions, pitch/reverse/pan/normalization/fades.
- Native `session/tests/stem_separation.rs`: real job/file capture/resampling,
  four-track batch/history/pool/save/reopen; cancellation/stale checks and
  persistent settings. Registered in `session/tests/mod.rs`.
- Mounted `analysis/panel.test.tsx`, `analysis/manifest.test.ts`, and
  `settings/stem-separation.test.tsx`: actual simulator document/history edits
  with an explicitly authored review fixture, runtime/default-model controls
  and manifest refusals. Simulator fixtures are lifecycle evidence only.

For verification with an operator-installed runtime and licensed pack, after
ordinary gates pass in the exclusive build window:

```powershell
$env:WINDFALL_ORT_DYLIB = 'C:\installed-runtime\onnxruntime.dll'
$env:WINDFALL_STEM_MODEL_PACK = 'C:\licensed-model-pack'
$env:WINDFALL_STEM_EVIDENCE = 'C:\evidence\windfall-stems' # optional
# Use the repo's documented MSVC setup / single Cargo job.
cargo test -p windfall-desktop real_onnx_four_stem_synthetic_mix_evidence -- --ignored --nocapture
```

The single new ignored test lives at
`apps/desktop/src-tauri/src/session/tests/stem_separation/real_ort.rs`.
It requires both environment paths and a real licensed pack; an absent
configuration fails rather than skipping silently. It uses actual native
import/job/apply and a twelve-second synthetic mix: kick-like bursts, bass
sine line, vowel-like harmonics, and chord pad. It writes source/mix/stem WAVs
and report JSON before quality assertions. Evidence defaults to a unique
`target/stem-evidence/<timestamp>-<pid>` folder.

The report includes pinned manifest/checksum/provenance, captured/output shape,
per-stem SDR against all four independently known sources, closest-source
identity, capture/worker/apply/total timings, reservations, and measured **process
peak working set** (Windows GetProcessMemoryInfo or Linux VmHWM). Memory includes
the harness, codecs, playback pool and foreign ORT allocations. Each stem must
have strictly higher SDR against its own source than every other source; this
checks role assignment and real separation rather than accepting copied mix.
The harness is deliberately a demanding quality gate: a model that cannot
separate these synthetic timbres fails and leaves its evidence. No arbitrary
quality tolerance is weakened for a selected model. Also evaluate real licensed
music and listening/alignment/stereo balance before claiming general quality.

## ort adapter source/API review

`ort = =2.0.0-rc.10`, MIT OR Apache-2.0, `default-features=false`, only
`load-dynamic`; no download-binaries or runtime/weights repository assets.
All calls are isolated in `windfall-analysis/src/ort_adapter.rs` behind
`SeparationBackend` / `PreparedSeparation`. The integrator checked the downloaded
rc.10 API and exercised it with a native runtime/model. The reviewed calls/types are:

1. `ort::init_from(path).with_name(...).commit()` and its loader/panic semantics.
2. `ort::session::Session::builder()`, `with_intra_threads(1)`,
   `with_memory_pattern(false)`, `commit_from_memory(&[u8])` and owned-weight
   lifetime. Session must retain any weight storage it requires after prepare.
3. `Session.inputs` / `outputs`, `.name`, `.input_type` / `.output_type`,
   `ort::value::ValueType::Tensor { ty, shape, .. }`,
   `ort::tensor::TensorElementType::Float32`, and dynamic dimension `-1`.
4. `ort::value::Tensor::from_array((Vec<usize>, Box<[f32]>))` with ndarray
   features off; shape conversions and owned value lifetimes.
5. `ort::session::SessionInputValue::from(tensor)`, named vector input conversion
   accepted by `session.run(Vec<(&str, SessionInputValue)>)`, and mutable session
   requirements. Adapt only this module if rc.10 uses a different input wrapper.
6. `SessionOutputs.get(&str)` and `try_extract_tensor::<f32>() → (shape, &[f32])`.
7. CPU-only execution defaults, Send/Sync requirements and library API version
   compatibility. No execution provider/package may download at build time.

Tests outside this adapter must remain unconditional; source review is not a
substitute for compiling and running them.
