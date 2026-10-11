# Additive, physical and drum instrument implementation — 2026-10-11

Branch: `worktree-agent-a10dd75da0e74d731`. Edit-only worker: no builds, tests,
benchmarks, generators or audio-device checks were run. Every test below is
**written, not executed**. Parity status is owned by the integrator.

## Row ledger

| Row | Implementation / claim | Remaining |
| --- | --- | --- |
| inst-boobass | Full-row candidate: Finger Bass now replaces its sole string on every positive note, including releases; 4 ms endpoint correction prevents retrigger discontinuities. Existing pluck, damping, pick position and two body resonances provide the bass-guitar tone contract. Six automatable controls retain descriptor indices. | Fresh integration execution and listening. |
| inst-harmor | Pending this stream. | Audio/image preparation and asset editor. |
| inst-morphine | Pending this stream. | Immutable harmonic snapshots and editor. |
| inst-ogun | Pending this stream. | Metallic spectrum and editor acceptance. |
| inst-harmless | Pending this stream. | Subtractive harmonic controls and editor acceptance. |
| inst-autogun | Pending this stream. | Full number domain and preset-only workflow. |
| inst-beepmap | Pending this stream. | Color/brightness image scan. |
| inst-drumaxx | Pending this stream. | Membrane/body model, 16-pad editor. |
| inst-bassdrum | Pending this stream. | Optional immutable sample layer. |
| inst-drumpad | Pending this stream. | Single physically modeled drum. |
| inst-fruity-drumsynth-live | Pending this stream. | Independent per-key programmable patches. |

## Written acceptance

- `physical/tests.rs::finger_bass_is_monophonic_and_old_key_release_cannot_release_new_string`: one held/releasing string under note storms, old-key note-off isolation, bounded retrigger continuity and stop silence.
- `windfall-engine/tests/inst_additive_physical_drum.rs::finger_bass_tone_reaches_history_disk_and_live_offline_pcm`: audible tone change, undo/redo across render block sizes, actual disk reopen, public live/offline exact PCM.
- `inspector/instrument-section.test.tsx`: mounted Finger Bass tone edit/undo and all six controls.

All are **written, not executed**. Existing physical tests independently measure
tuning, decay, damping/body harmonic balance, pick position, extreme loop
stability, deterministic reset and partition invariance.

## Integrator commands

In Git Bash after `source scripts/msvc-env.sh`, redirect `TS_RS_EXPORT_DIR` to a
fresh temporary native path, then run:

```sh
cargo test -p windfall-dsp --lib physical
cargo test -p windfall-engine --test inst_additive_physical_drum
cargo test -p windfall-project --lib --tests
# Regenerate bindings/descriptors and simulator using the repository workflow.
pnpm --dir apps/desktop test src/features/channel-rack/inspector/instrument-section.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

## Integration notes and risks

- No new dependency, generated file edit, or parameter index change.
- The new engine test is an automatically discovered Cargo integration target.
- Inspect mono retrigger continuity and tone-difference threshold first; no runtime evidence is claimed.
- Shared seams touched so far: none.
