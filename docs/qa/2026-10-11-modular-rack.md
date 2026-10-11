# Modular rack and note modules — implementation hand-off

Branch: `gpt/w3-modular-rack`. Work is edit-only. No build, test, package
runner, benchmark, device check or generated-artifact regeneration was run.

## Current implementation

- Saved channel graph side table on ProjectSettings, omitted from legacy
  defaults. Stable graph-local IDs and ports; typed checked edges; cycle,
  depth/node/edge/event limits; atomic replacement/history and channel cleanup.
- DSP note-map foundations registered; six saved note-module configurations.
- Control-side rack compilation and preallocated frame processing are being
  integrated. Internal audio wires include delay compensation; note wires
  retain parent and child note-instance ownership. Nested built-in graphs are
  flattened. Control ports are external inlets, not duplicated control sources.

## Acceptance status

All eight rows remain partial until integration and the tests below are
written and executed. In particular hosted-node state/editor capture is not
implemented: native-bound channels reject rack replacement explicitly.

## Integrator commands (not executed here)

In Git Bash, `source scripts/msvc-env.sh`; put node/pnpm on PATH; set
`TS_RS_EXPORT_DIR` to a task-specific temporary directory.

- `cargo test -p windfall-dsp --lib notemap`
- `cargo test -p windfall-dsp --lib rack_note`
- `cargo test -p windfall-project --test modular_rack`
- `cargo test -p windfall-engine --test modular_rack`
- `cargo test -p windfall-engine --test engine`
- `cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine --all-targets -- -D warnings`
- Regenerate Rust/TS bindings and document WASM with the repository scripts.
- `pnpm --dir apps/desktop test src/features/modular src/lib/ipc/sim/document.test.ts --maxWorkers=4`
- Desktop typecheck/lint; plugin state/capture regression suites.

## Generation

New rack graph/node/edge/endpoint/processor/preset/channel-rack and note-module
types; ProjectSettings.modularRacks; Command.setChannelRack. Descriptors remain
append-only; no generated files are edited by this worker.

## Risks and remaining gates

Runtime integration, mounted purpose-built editor, acceptance tests and exact
shared seam inventory will be recorded here as each coherent piece lands.
Hosted processor lifecycle and capture, native platform listening and executed
acceptance stay explicit gates. Do not promote tracker rows from this document.
