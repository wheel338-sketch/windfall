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
  page with faders, pads and keyboard. Settings now exposes explicit enable,
  bind address, pairing instructions and disable. Global connection limits,
  read deadlines, revocation and bounded held-note release are implemented.
- `fmt-cloud-backup`: provider trait, folder history with SHA-256, immutable
  version objects, expected-head conflict check, cooperating-writer lock,
  chunked cancel, retention and no-overwrite verified restore. Session captures
  the existing portable archive including plugin/audio state. Browser Cloud
  backup tab exposes folder selection, manual backup, history, refresh, cancel,
  retention and verified restore through the normal project-open flow.
  Per-project opt-in backup after ordinary Save is wired; original native
  generation is carried through capture so replacement cannot upload a new song.
- `core-loop-starter`: eight original two-bar synthesized CC0 genre loops with
  metadata/provenance, deterministic native generator, tempo/key resynthesis,
  guarded source/clip insertion through a single undo batch. Browser Loops tab
  filters genre, source key and tempo, auditions, inserts, and drags a guarded
  loop payload into the playlist. Derived cache WAVs get provenance sidecars.

## Tests written, not executed

- `backup::tests::checksum_conflict_cancel_retention_and_restore_round_trip`
- `remote_protocol::tests::{rfc6455_handshake_reference,masked_text_and_limits}`
- `session::phone_remote::tests::authentication_replay_expiry_and_rate_limits`
- `windfall_factory::loops::tests::{byte_stable_generation_and_metadata,tempo_fit_preserves_bars_and_key_fit_changes_pitch}`
- `session::tests::remote_cloud_content::{remote_named_command_undo_stale_and_recording_guards,loopback_pair_websocket_fader_and_undo,backup_portable_project_restore_round_trip_and_original_generation,loop_fit_insertion_one_undo_save_reopen_and_provenance,rendered_loop_cannot_attach_to_replaced_or_recording_document}`
- Mounted UI: `features/settings/phone-remote.test.tsx` (explicit enable/pair/
  disable and native capability error), `features/backup/panel.test.tsx` (explicit
  folder/manual backup, conflict/cancel, opt-in after-save generation/head),
  `features/loops/browser.test.tsx` (filter/audition/drop payload and insertion call).

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
8. `cargo test -p windfall-desktop --lib session::tests::remote_cloud_content`
9. `pnpm --dir apps/desktop test src/features/settings/phone-remote.test.tsx src/features/backup/panel.test.tsx src/features/loops/browser.test.tsx --maxWorkers=4`
10. `pnpm --dir apps/desktop typecheck`; `pnpm --dir apps/desktop lint`.

Only `rustfmt --edition 2024` on the new Rust modules has run (success); this
is formatting, not compilation or acceptance evidence.

## Current row claims and remaining work

| Row | Claim | Remaining |
| --- | --- | --- |
| `core-fl-studio-remote` | Full-row software candidate, unverified | Integrator compilation/tests and real phone/LAN/firewall checks. HTTP is local plain text, with no TLS or Internet binding. |
| `fmt-cloud-backup` | Partial; folder-provider software candidate | User-selected real provider/account and adapter, plus synced-drive/remote CAS validation. Automatic hook currently covers ordinary Save, not Save new version. |
| `core-loop-starter` | Full-row software candidate, unverified | Factory asset regeneration, compilation/tests and listening/cross-platform byte checks. All v1 melodic loops are minor; fit is on insertion, not automatic after later project-tempo changes. |

Shared seams touched: native `commands.rs`, `lib.rs`, `session/mod.rs`,
`session/tests/mod.rs`, `session/archive.rs`, `session/transport.rs`;
`windfall-ipc/src/lib.rs`, factory `src/lib.rs`; frontend IPC
`backend.ts`, `tauri.ts`, `mock.ts`, browser `index.tsx`, playlist `grid.tsx`,
settings `settings-dialog.tsx`, and ordinary Save flow `lib/flows/project.ts`.
No new project Command variants, descriptor indices, persisted Project fields,
DSP callback work or generated binding/WASM edits were introduced.

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
