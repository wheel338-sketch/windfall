# Round-seven acceptance — 2026-10-13

Rows: `wf-languages`, `wf-project-settings`, `fmt-flp-import`. Main checkout,
branch `feat/parity-100`, started at `eca4de0d`. One build or test process at
a time through `target/round7f/run.sh` (Rust: MSVC environment,
`CARGO_BUILD_JOBS=8`, an isolated `TS_RS_EXPORT_DIR`, the native fixture
variables) and `target/round7f/front.sh` (Node and pnpm on PATH). Logs are
`target/round7-final-<step>.log`. No sub-agents, no audio device, no ignored
hardware test, no new dependency, no branch, push, pull request or tag.
`docs/parity/parity.json` and `PARITY.md` are untouched.

## Running record

### A second session was still building when this one started

This session was started at 19:12 on the premise that the round-seven builder
had finished and the tree was clean at `eca4de0d`. It had not: its closing
gate run (`target/round7/gates4.sh`, started 19:12:57) was in flight and it
had uncommitted edits in `crates/windfall-flp`, `docs/flp/coverage.md`, its
record and the simulator. What happened, in order:

- 19:13: `git merge --no-ff opus/r7-languages` committed as `579c40a9`
  (531 files, frontend only, no conflicts). The merge commit contains none of
  the builder's uncommitted files.
- 19:12:30 to 19:13:20: `pnpm typecheck` (exit 0) and `vitest src/lib/i18n`
  ran here while the builder's `cargo build` gate step was running. These are
  the only commands of this session that overlapped another build.
- 19:15: the overlap was noticed from `git status` and the process list. From
  then until the builder reported its commit, this session ran no build or
  test and edited no source in the checkout; its own uncommitted step-one
  edits were set aside as a patch (`target/round7f/step1-parked.patch`) so the
  builder's frontend gates saw the committed merge only. The two sessions
  agreed this by message.
- 19:47: the builder's gates finished and it committed `24117f6a` (its own
  paths only: omitted import controls filed as dropped in the review, with a
  test and the rebuilt simulator). It reported that it starts no further
  build. Its numbers for that source: `cargo test --workspace` 4838 / 0 / 40;
  Vitest 6495 / 2 over 534 files on the merged tree (the two translation
  guards). From here on this session is the only builder.

Consequence for the record: the builder's `gates4` frontend numbers describe
the merged tree at `579c40a9` plus its own files, not the tree it started
from. Nothing of this session was verified by the builder's run.

### Step 1: translation branch merged, guards green

Merged `opus/r7-languages` (`579c40a9`): no textual conflicts. On the merged
tree the two guards reported exactly what main had added since the branch's
base (`target/round7-final-i18n1.log`): three strings in
`features/channel-rack/voice/voice-panel.tsx` and 813 descriptor addresses per
language (473 distinct labels).

What was added:

- **Voice panel.** `Delay` and the delay knob's accessible name go through the
  catalog (`{title} delay`), and the restart switch is one parameterised
  message, `Restart {target} LFO on each note`, with the target translated as
  the neighbouring `Enable {target} LFO` does. The section title was built as
  `Pitch` + ` LFO` from the target's id and stayed English in Spanish; it is
  now one whole catalog entry per target (`Pitch LFO` → `LFO de afinación`),
  which also fixes the accessible names made from it. English text is
  unchanged.
- **Three-oscillator synth.** Fifteen entries in `descriptors-instruments.ts`
  (pulse width, octave layer and two-octave layer per oscillator, the filter's
  band-pass and high-pass levels and the four short forms).
- **Multiband delay.** The descriptor display path looks a name up by its
  exact English text (`translatedText`, and the guard checks
  `Object.hasOwn(catalog, name)`), so a parameterised entry is not supported
  and each of the 384 names is its own key. They are generated, not written
  out: `messages/descriptors-band-delay.ts` builds the 384 names, the 48 short
  forms the generic editor derives (`1 delay`), its 24 group headings
  (`Bank 1 time ms`) and the two layout choices from one sentence per property
  and language, with the key type kept exact through template-literal types
  (so `MessageKey` stays a closed union). Spanish: `Retardo de la banda 3 del
  banco 2`, `Nivel …`, `Panorama …`; headings `Tiempos del banco 1 (ms)`,
  `Niveles del banco 1`, `Panoramas del banco 1`; `16 bandas`.
- **`inspector-messages.ts`.** Two Spanish entries held U+FFFD where accented
  letters had been lost (`El par�metro {id} no est� disponible…`, `…no est�
  disponible. La ruta se conserva.`); both are restored. A scan of every
  catalog module for U+FFFD, UTF-8 read as Latin-1 and a question mark between
  letters finds nothing else (`target/round7f/mojibake.py`). The file had one
  stray CRLF (its last line); it is LF throughout now.

One test-harness change, written down because the rule is "fix the product,
not the test": `whole-app-coverage.test.ts`'s whole-application walk parses
about a thousand files with the TypeScript compiler and takes 5.8 s here on
its own, against Vitest's default limit of 5 s. Beside other workers it timed
out instead of reporting (`target/round7-final-i18n3.log`). It now has an
explicit 60 s limit. No assertion, pattern or invariant entry changed.

Commands for this step, on the merged tree with the additions above:

| Command | Result | Log under `target/` |
|---|---|---|
| `pnpm --dir apps/desktop test src/lib/i18n src/features/channel-rack/voice --maxWorkers=4` | 10 files, 37 passed, 0 failed (both guards) | `round7-final-i18n4.log` |
| `pnpm --dir apps/desktop typecheck` | pass, 23 s | `round7-final-s1-typecheck.log` |
| `pnpm --dir apps/desktop lint` | pass, 36 s | `round7-final-s1-lint.log` |
| `pnpm --dir apps/desktop test --maxWorkers=4 --reporter=default --reporter=json --outputFile=../../target/round7-final-s1-frontend-results.json` | 534 files, 6,497 passed, 0 failed, 0 skipped, 562 s | `round7-final-s1-frontend.log` |
