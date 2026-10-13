# Round-six preset rendering hand-off — 2026-10-13

Row: `fmt-import-legacy-presets`.

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r6-preset-render`.
Branch: `gpt/r6-preset-render`.
Base: `0b72d3f218d4f59639e055053301006170f46850`, as overridden by the task.

Implementation is committed; integration validation is pending. This edit-only
worker does not claim the row built, tested, or accepted.

Commits before this hand-off:

- `d0b45d60127d0c12d3343d065b5df9450deda741`: engine render, speech preset
  persistence, native jobs, shared source/WAV ownership and Rust tests.
- `51242988eeaa5ee40305253e76b8078d0a129e71`: mounted render dialog, actual
  browser/drop routes, retained-source re-render control, IPC adapters and UI tests.
- `d4fc906ae118ede564958c6273dd1dcbaf2c8d8a`: full live/offline PCM assertions,
  including the release interval rather than a pre-note-off prefix.

## Scope decision and sources read

Read the supplied worker contract in full, round-five acceptance, roadmap
section 3, QA completion decisions, `2026-10-13-r5-interchange.md`, and
`2026-10-13-r6-formats.md`, including its SYN/speech search evidence. All
repository work after the explicitly requested contract read used this worktree.
The installed shadcn skill was applied through existing local components; the
contract's prohibition on package commands/downloads was honored.

The overseer's exclusion of proprietary `.syn` and `.speech` files stands.
Neither source is inspected or parsed. The explicit native report and mounted
dialog direct users to Windfall synthesizer `.wfpreset` files and Text Voice
channel `.wfpreset` files. No vendor content, sounds, binary inspection, or
reverse engineering was used.

## What changed

- `crates/windfall-engine/src/preset_render.rs`, registered in `src/lib.rs`:
  a bounded worker/control-side preset renderer. It reads the existing Windfall
  preset envelope, constructs a private project, prepares instrument data, and
  runs `render_reporting`, hence the real instrument DSP, mixer and PDC. It
  accepts built-in instrument plugin presets and built-in channel presets;
  channel level, pan, voice, timing and expression settings are carried into
  the render project. Hosted processors and modular rack presets get an explicit
  capability refusal. No realtime processor implementation changed.
- The capability table accepts 29 synthesizer kinds, including SubtractiveSynth,
  CompactSynth, PocketSynth and the FM, additive, poly and spectral families.
  SpeechVoice phoneme presets and TextVoice channel text recipes also render.
  Empty text/phrases have specific errors. Speech retains the complete phrase,
  extending the held note when necessary, including TextVoice tune/key playback
  speed. Release time is rendered up to the chosen limit with automatic silence
  termination. Mono is the arithmetic mean of the rendered stereo channels.
- Bounds: renderable Windfall preset input 1 MiB; public DrumSynth input remains
  64 KiB; MIDI note 0–127, velocity 0–1, note length 0.01–60 seconds, release limit
  0–30 seconds, project rate 8–192 kHz, natural phrase body at most 240 seconds,
  stereo render PCM budget 128 MiB. Length uses the engine's tick resolution at
  60 BPM, rounded upward to cover the requested time. No dependency was added.
- `crates/windfall-project/src/{preset,model,command,lower,check,lib}.rs`:
  Text Voice channel presets now carry their versioned text/pitch/duration
  recipe and instrument controls, with generated project PCM references cleared
  during capture. Existing nonportable sample banks remain outside this feature.
  `ProjectSettings.preset_renders` retains source/result sample IDs and render
  settings. `RegisterPresetRender` is appended to the command enum. New fields
  have serde defaults. Checked command/file invariants reject missing, duplicate,
  cyclic and raw-source-as-PCM references; deleting a retained source is refused,
  and deleting its rendered sample removes its relation in the same undo step.
- `apps/desktop/src-tauri/src/session/preset_render.rs`, registered in
  `session/mod.rs`: begin/cancel/render IPC service, one claim per ticket,
  supersession, and generation/edit/replacement/rate/source guards. Cancellation
  reaches the engine renderer and sample-preparation ticket. Cancellation and
  final import installation share the job mutex. DSP and filesystem work run
  on the blocking IPC worker without document/audio locks. A retained source
  may be selected by sample ID after save/reopen, without resolving project paths
  in the frontend.
- `session/{preset_assets,library,sampler_processing,samples,files}.rs`:
  DrumSynth and Windfall imports share rendering preparation, bounded original
  reading, staged source/WAV ownership, candidate sample preparation, one checked
  undo transaction and public created-ID ordering. Both files publish without
  replacing an existing target. RAII cleanup deletes only files published by
  the failed job. Preset import names use the original source stem. The sampler's
  root key follows the chosen render note. First-save and save-as carry the
  `.wfpreset` and WAV together and relink normal history paths. Raw retained
  `.wfpreset` source assets are not submitted to background PCM loading.
- `crates/windfall-codec/src/{drumsynth,drumsynth_model}.rs`: additive
  `render_cancellable`, with cancellation at each existing bounded synthesis
  block. The original render delegates with an always-continue callback; its
  numerical model and existing tests are preserved.
- Native `src/{commands,browser,samples}.rs`: authenticated command registration,
  browser classification of the preset routes, bounded Windfall preset preview
  through the engine, and proprietary-source refusal before file decoding.
- `apps/desktop/src/features/preset-render/{dialog,types}.tsx/ts`:
  accessible shadcn/Base UI render dialog, note/velocity/length/tail validation,
  read-only project rate, mono/stereo choice, visible working/cancellation/errors,
  unsupported-source message, and saved-source re-render button. A failed job
  requires reopening to capture a fresh ticket. Project replacement and history
  navigation close/cancel the dialog; late results are discarded.
- UI `features/browser/commands.ts`, `features/channel-rack/channel-ops.ts`,
  `features/channel-rack/inspector/sample-section.tsx`, and
  `features/layout/app-shell.tsx`: actual browser add/replace and sampler/rack
  drop/file-picker operations open the dialog; captured file tokens and channel
  destinations survive selection changes. The sampler inspector exposes
  **Re-render preset…** and excludes raw recipe assets from its sample chooser.
- `src/lib/ipc/{backend,tauri,mock}.ts`: new runtime IPC methods and file filters.
  Browser mock reports unavailable instrument DSP instead of importing fake audio.
  Generated bindings and simulator artifacts were not edited or generated.

Use the existing **Save channel preset…** action for Text Voice after entering
and rendering its text in the instrument editor. Import that `.wfpreset` from
the browser or drop it on a sampler, set render controls, and choose **Render
as sample**. Instrument plugin presets can be imported through the same route.

## Exact new tests — written, registered, non-ignored, not run

`crates/windfall-engine/tests/preset_render.rs` is an automatically discovered
Cargo integration target:

- `compact_and_subtractive_presets_equal_live_engine_pcm_at_project_rates`
- `phoneme_and_text_speech_presets_equal_live_engine_pcm`
- `every_accepted_synth_kind_has_finite_deterministic_measured_pcm`
- `mono_note_velocity_and_release_controls_measure_actual_pcm`
- `cancelled_hostile_oversized_and_unsupported_presets_never_return_partial_pcm`

These compare full public live PCM after PDC to the preset render, including
release; compare full renders at irregular block sizes; measure nonzero finite
PCM for every accepted synth kind; assert exact mono arithmetic, release energy,
note/velocity changes, silence at zero velocity, cancellation and input budgets.

`crates/windfall-project/tests/presets.rs`, existing registered target:

- `speech_channel_preset_retains_text_recipe_discards_generated_pcm_and_has_legacy_defaults`
- `rendered_preset_source_links_have_checked_history_serialization_and_legacy_defaults`

`crates/windfall-codec/tests/drumsynth_model.rs`, existing registered target:

- `cancellable_drumsynth_matches_full_pcm_and_never_returns_a_cancelled_prefix`

`apps/desktop/src-tauri/src/session/tests/preset_render.rs`, explicitly registered
in `session/tests/mod.rs`:

- `windfall_browser_and_sampler_drop_render_pcm_source_history_and_portable_reopen`
- `speech_channel_presets_save_text_and_render_the_complete_phrase`
- `cancelling_before_dsp_after_render_and_before_commit_preserves_project_history_pool_and_files`
- `stale_preset_jobs_reject_edits_replacement_source_changes_rate_changes_and_supersession`
- `a_render_ticket_is_claimed_once_and_browser_root_revocation_refuses_installation`
- `malformed_oversized_empty_speech_and_proprietary_sources_fail_without_import_side_effects`

Native fixtures exercise checked browser import and ordinary sampler drop,
project-rate exact WAV PCM, source/result ownership and root key, one-step history,
undo/redo, original deletion, first save, save-as, reopen and re-render by retained
source ID. Barrier tests compare document/history, pool allocation identities and
owned files around cancellation and stale work; source replacement includes
identical bytes in a different file identity. Duplicate claims and browser root
revocation are refused.

`apps/desktop/src/features/preset-render/dialog.test.tsx`, Vitest discovery:

- `mounts editable note, velocity, length, tail, project rate and mono/stereo controls and sends real import settings`
- `browser import carries its pinned token and sampler drop keeps its captured destination when selection changes`
- `cancel dismisses a pending native job and rejects its late result without changing project or history`
- `project replacement cancels a dialog and native capability/empty-text failures remain visible`
- `shows the explicit proprietary .%s message and Windfall equivalents without starting DSP` (both `syn` and `speech`)
- `browser mock reports unavailable DSP instead of a successful render`
- `mounted rerender control uses the retained source identity and saved controls`

## Integrator gates

Run serially in the integration checkout with the normal MSVC environment and
an external `TS_RS_EXPORT_DIR`. Regenerate bindings and WASM there before mock
UI checks. Relevant added binding types/fields are `PresetRenderSettings`,
`SamplePresetRender`, `ChannelPreset.sampleInstrument`,
`ProjectSettings.presetRenders`, and appended `Command.registerPresetRender`.

- `cargo test -p windfall-codec --test drumsynth --test drumsynth_model -- --test-threads=1`
- `cargo test -p windfall-project --test presets`
- `cargo test -p windfall-engine --test preset_render`
- `cargo test -p windfall-desktop --lib session::tests::preset_render -- --test-threads=1`
- `cargo test -p windfall-desktop --lib session::tests::import_formats -- --test-threads=1`
- Existing session sampler-processing, import-recording, sample/file/save-as,
  caller-lease, library and project-preset suites.
- `pnpm --dir apps/desktop test src/features/preset-render/dialog.test.tsx src/features/browser/browser-use.test.tsx src/features/channel-rack/file-picker-generation.test.ts --maxWorkers=4`
- Existing project-files/preset tests, desktop typecheck/lint, scoped strict
  Clippy, `cargo fmt --all --check` after integration formatting, then full gates.

Allowed checks performed: source/diff review, `git diff --check` (clean), and
single-file `rustfmt --check --edition 2024 --config skip_children=true` on changed
Rust files. Formatter output reported formatting differences in most files, with
no syntax errors reported. This is neither typecheck nor test evidence; format
the files during integration. Commit hooks were disabled to avoid implicit builds.

Nothing was built or run as an application/test/audio render. No cargo, package
runner, test runner, generator, install, download, subagent, push, PR or tag ran.
No new dependencies, parity tracker edits, generated bindings or simulator
artifacts are included; `crates/windfall-flp` was not touched.

## Completion and least certain spots

SOFTWARE: No missing behavior identified within the overseer's Windfall-own-preset
scope. Compilation, generated-artifact integration and test execution remain
required acceptance gates, not claimed passing evidence from this edit-only round.

EXTERNAL-ONLY: Musical listening of the original Windfall synth and robotic English
voice output after software gates pass. Proprietary SYN/speech support is excluded
by the explicit overseer decision and is not an unimplemented reader promise.

Least certain spots:

- Full live versus offline release equality uses public Processor reopen and
  linear Song playback after host PDC. The assertions are deliberately exact;
  investigate any stop/tail discrepancy rather than loosening tolerances.
- The table covers default patches of all 29 accepted synth kinds. Check that
  every default spectral/sequence patch satisfies its measured nonzero assertion.
- New persisted preset fields and command variant require fresh bindings/WASM.
  Check existing exhaustive/snapshot fixtures and native preset-save UI paths.
- Windows tempfile Float32 WAV writing followed by `persist_noclobber`, and
  cancellation/stale refusal through sample-preparation/publication, need native
  execution. Existing DrumSynth import/save/reopen fixtures must still pass.
- The mounted tests depend on Base UI dialog/toggle semantics and asynchronous
  project-store events. Native PCM is measured in Rust, not simulated by these
  mounted test doubles. Formatting/type/lint failures must be repaired before
  promoting the row.

HAND-OFF COMPLETE
