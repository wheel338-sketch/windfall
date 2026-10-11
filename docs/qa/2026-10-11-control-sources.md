# Internal control sources — implementation hand-off

Branch: `gpt/w3-control-sources`, based on `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.

Edit-only worker: **no builds, tests, package tools, device checks, or generated artifact updates run**. All acceptance additions below are written, not executed. Work is confined to this worktree.

## Current implementation

- Registered control-only Keyboard Source and Formula Source, reusing existing bounded DSP. Formula adds input c at the end of its descriptors (old parameter indices stay stable).
- Added optional project configuration for note input, drawn envelope, formula text, peak/RMS and tempo sync, and up to eight named surface widgets. Checked command edits the configuration as one settings/history transaction.
- Internal links now validate source existence/ports, reject direct and indirect source-parameter cycles and conservative detector audio feedback. Source deletion removes its links in the existing command transaction; undo restores links and source together.
- Engine hosting and purpose-built editors are in progress. No row is currently claimed complete.

## Integrator commands (not run here)

Use Git Bash, `source scripts/msvc-env.sh`, and set `TS_RS_EXPORT_DIR` to a task-specific temporary native path before Rust tests.

```sh
cargo test -p windfall-dsp --lib control
cargo test -p windfall-project --test controller_links
cargo test -p windfall-project --test internal_controllers
cargo test -p windfall-engine --test engine internal_controllers
# After integrating and generating Rust bindings/descriptors/WASM:
pnpm --dir apps/desktop test src/features/internal-controllers --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

## External gates

Real DC-coupled outputs, calibration and voltage safety are hardware gates; no hardware behavior or safe voltage is claimed. MIDI hardware requires real device checks.
