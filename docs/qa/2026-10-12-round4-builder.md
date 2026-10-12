# Round-four builder — 2026-10-12

Base: `cfd2d37ee5af6ce8dc3f55acd539ba4929b0945b`, branch `feat/parity-100`.
Serial builds/tests only; Git Bash with `scripts/msvc-env.sh`, Cargo jobs 8,
temporary native `TS_RS_EXPORT_DIR`. No worktree changes or hardware tests.

- **A1 fixed:** adjacent 5 ms RMS cells mistook beating between held chord
  tones for attacks (27 false onsets), splitting otherwise accurate pitches.
  Compare novelty against the preceding 40 ms envelope peak. Existing tests
  unchanged. `cargo test -p windfall-analysis --test classical_tools --
  --test-threads=1`: **8 passed / 0 failed / 0 ignored**;
  `target/round4-builder-a1-fixed.log`. Baseline 0/1/0 and diagnostic probe:
  `target/round4-builder-a1-baseline.log`, `target/round4-builder-a1-probe.log`.
  Temporary instrumentation removed.

- **A2 fixed:** desktop's 256-frame helper block could be submitted and expire
  inside one 480-frame callback, forcing latency-aligned dry fallback even for
  healthy native PCM. Desktop bridge now uses 512-frame helper blocks (existing
  protocol capacity); two-block pipeline/PDC reports the resulting latency,
  with no callback waits/allocations. Existing assertions unchanged.
  `cargo test -p windfall-desktop --lib session::tests::mixer_preset_native --
  --test-threads=1 --nocapture`: **2 passed / 0 failed / 0 ignored** including
  capture, undo/redo, restore and reopen, all-frame PCM and allocator checks;
  `target/round4-builder-a2-block-probe.log`. Baseline **0/2/0**:
  `target/round4-builder-a2-baseline.log`.

- **A3 fixed:** portamento allocated a second overlapping voice; continue the
  most recent compatible held occurrence and transfer logical ownership instead.
  Slide used its note length as the ramp length; use `glide_ticks` in the
  musical clock. `cargo test -p windfall-engine --test piano_phrase_pcm --
  --test-threads=1 --nocapture`: **1/0/0**;
  `target/round4-builder-a3-engine-fixed.log`. Native joined expression gate:
  `cargo test -p windfall-desktop --lib
  session::tests::plugins::piano_expression_native_helper_sequences_slide_portamento_and_midi_only_bends
  -- --test-threads=1 --nocapture`: **1/0/0**, all four CLAP/MIDI-only and
  Slide/Portamento cases; `target/round4-builder-a3-native-current-fixed.log`.
  Two fixture expectation corrections, with unchanged numerical assertions:
  the ramp fixture now enables a zero-time gate envelope, because the explicit
  existing `without_an_envelope_the_whole_sample_plays_whatever_the_note_length`
  contract makes the old gap expectation false; the native stop fixture drains
  its advertised 32-frame delay before checking sustained silence. A causal
  key-up at frame 0 leaves the previous 32 samples in that delay, so including
  them in a 4096-frame RMS can exceed 0.01 without any stuck note.
  Initial native probes accidentally selected the old 2026-10-10 DLL; rebuilt
  current fixture offline in `target/plugin-session-fixtures` and corrected
  runner. `target/round4-builder-fixture-current.log`. Earlier native failures
  remain logged as diagnostic evidence, not current-fixture product verdicts.
  Neighboring `cargo test -p windfall-engine --test engine expression --
  --test-threads=1`: **2/0/0**, `target/round4-builder-a3-expression-regression.log`.

- **A4 correctness fixed:** pitch-synchronous
  window period-count rounding jumped by whole periods as the tracker jittered,
  adding crossings; use a continuous complete two-period span. The ~6 kHz
  detector's high-frequency lag interpolation lost narrow harmony peaks;
  raise resolution to ~8 kHz while retaining the existing public/PDC delay.
  LPC order/window/update cadence and stability loading were fixed in sample
  units; scale their physical support with rate (bounded at order 128/window
  4096), preserving baseline behavior through 48 kHz. All test tolerances and
  assertions unchanged. Pitch units **3/0/0**:
  `target/round4-builder-a4-span-probe.log`; harmony and latency/heap checks
  **2 passed**, formants still failed before support fix:
  `target/round4-builder-a4-quality-rate-fixed.log`. Full nonaligned pitch and
  independent formants check across all six rates and both modes **1/0/0**,
  121.64 s: `target/round4-builder-a4-formant-resolution.log`.
  Full spectral units **23/0/0**, `target/round4-builder-a4-spectral-fixed.log`.
  High-rate model CPU budget remains to be measured in B.

- **A5 fixed:** queued sequenced echoes survived pattern loops, while Seek
  reset all echo metadata and released generated live owners. Emit a bounded
  pass-boundary control trigger before the next pass's note-ons and cancel
  only sequenced echoes there. Seek retains live echo queues/owners; arpeggio
  scheduling is rebuilt as before. `cargo test -p windfall-engine --test engine
  rack_voice -- --test-threads=1 --nocapture`: **27/0/0**, including original
  unchanged seek/loop PCM comparison and callback allocator/partition gates;
  `target/round4-builder-a5-fixed.log`. Baseline **0/1/0**:
  `target/round4-builder-a5-baseline.log`. Internal sequencer note-collection
  test helpers now distinguish the new control trigger, verify the two exact
  pass-boundary frames, and retain every note-list assertion; they previously
  assumed all trigger variants were note-ons in this fixture.
  `cargo test -p windfall-engine --lib sequencer -- --test-threads=1`:
  **10/0/0**, `target/round4-builder-a5-sequencer-fixed.log` (first run 8/2/0
  identified those helper assumptions; no note/frame expectations relaxed).

- **A6 fixed:** capture trimmed 12 frames of warm-up/group delay without
  offsetting launch admission. Source attack at input frame 1532 therefore
  appeared at output frame 1520 (peak 0.000154 before requested frame 1531).
  Offset recorded control frames by the trimmed warm-up and preserve stable
  same-frame request order. `cargo test -p windfall-engine --test performance
  -- --test-threads=1 --nocapture`: **4/0/0**, unchanged assertions;
  `target/round4-builder-a6-fixed.log`. Baseline **0/1/0**, diagnostic
  `target/round4-builder-a6-baseline.log`, `target/round4-builder-a6-probe.log`;
  temporary instrumentation removed.

## Resume / final gates

B is next, then A7. Release budgets and final gates not yet run.
