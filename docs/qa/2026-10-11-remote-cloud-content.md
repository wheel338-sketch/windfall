# Remote, optional backup and own loop content — implementation hand-off

Branch: `gpt/w3-remote-cloud-content`, based on `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
This worker edits only. No builds, tests, package runners, generators, benchmarks
or hardware checks have been executed. All tests listed below are **written,
not executed**. Parity status is owned by the overseer and remains untouched.

## Current implementation

- `core-fl-studio-remote`: specific private IPv4 bind only when enabled,
  bounded std::net HTTP/WebSocket, one-use five-minute pairing code, random
  256-bit eight-hour session token, increasing sequences, per-token rate and
  connection/message limits, same checked fader Command and guarded transport
  path as UI, live-note API, cached existing realtime feed, responsive phone
  page with faders, pads and keyboard. Desktop settings wiring is in progress.
- `fmt-cloud-backup`: provider trait, folder history with SHA-256, immutable
  version objects, expected-head conflict check, cooperating-writer lock,
  chunked cancel, retention and no-overwrite verified restore. Session captures
  the existing portable archive including plugin/audio state. UI is in progress.
- `core-loop-starter`: eight original two-bar synthesized CC0 genre loops with
  metadata/provenance, deterministic native generator, tempo/key resynthesis,
  guarded source/clip insertion through a single undo batch. UI is in progress.

## Tests written, not executed

- `backup::tests::checksum_conflict_cancel_retention_and_restore_round_trip`
- `remote_protocol::tests::{rfc6455_handshake_reference,masked_text_and_limits}`
- `session::phone_remote::tests::authentication_replay_expiry_and_rate_limits`
- `windfall_factory::loops::tests::{byte_stable_generation_and_metadata,tempo_fit_preserves_bars_and_key_fit_changes_pitch}`

## Integrator commands (not run by this worker)

Use Git Bash, `source scripts/msvc-env.sh`, `CARGO_BUILD_JOBS=1`, an isolated
target folder, and a temporary `TS_RS_EXPORT_DIR`.

1. Regenerate bindings (`scripts/gen-bindings.sh <task-output-directory>`) and
   regenerate factory content: `cargo run -p windfall-factory --release -- generate content/factory`.
   The edit-only worker cannot generate WAVs. Eight loop WAVs and
   `Loops/manifest.json` are intentionally pending. Native loop browser code
   also renders on demand, so runtime insertion does not depend on those files.
2. `cargo test -p windfall-factory`
3. `cargo test -p windfall-desktop --lib backup`
4. `cargo test -p windfall-desktop --lib remote_protocol`
5. `cargo test -p windfall-desktop --lib phone_remote`
6. `cargo test -p windfall-desktop --lib session::tests::archive`
7. `cargo clippy -p windfall-factory -p windfall-ipc -p windfall-desktop --all-targets -- -D warnings`

Generated types: `RemoteSettingsState`, `BackupVersion`, `LoopEntry`, `LoopCatalog`.
No generated files were edited. Cargo.lock needs normal integrator reconciliation
for direct reuse of already locked `sha2 = 0.10.9` and `getrandom = 0.3.4`.
No HTTP/server/cloud SDK dependency was added.

## Known risks / external gates

Uncompiled Rust/TypeScript seams and strict lint first; bounded server wire and
shutdown/recording races next; then archive backup round trips and musical fit.
Real phones/tablets, LAN/firewall behavior, synced-folder conflict/power-loss
behavior, cross-platform byte identity and listening review remain external.
Folder locks coordinate a single filesystem, not disconnected synced-drive
replicas. A cloud adapter must provide actual remote compare-and-publish.
