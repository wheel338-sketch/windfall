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
  Phone faders reflect desktop edits between gestures; channel selection is
  retained across feed-driven rebuilds and reconnect clears in-memory authority.
- `fmt-cloud-backup`: provider trait, folder history with SHA-256, immutable
  version objects, expected-head conflict check, cooperating-writer lock,
  chunked cancel, retention and no-overwrite verified restore. Session captures
  the existing portable archive including plugin/audio state. Browser Cloud
  backup tab exposes folder selection, manual backup, history, refresh, cancel,
  retention and verified restore through the normal project-open flow.
  Per-project opt-in backup after ordinary Save and Save new version is wired; original native
  generation is carried through capture so replacement cannot upload a new song.
  UI preparation also rejects replacement during the native identity request;
  withdrawal of the opt-in prevents a pending automatic backup from launching.
- `core-loop-starter`: eight original two-bar synthesized CC0 genre loops with
  metadata/provenance, deterministic native generator, tempo/key resynthesis,
  guarded source/clip insertion through a single undo batch. Browser Loops tab
  filters genre, source key and tempo, auditions, inserts, and drags a guarded
  loop payload into the playlist, including blank destination rows in the same
  undo batch. Derived cache WAVs get provenance sidecars.

User workflows, protocol bounds, provider adapter transaction contract, account
decisions and per-asset provenance are documented in
[`docs/REMOTE-BACKUP-LOOPS.md`](../REMOTE-BACKUP-LOOPS.md). The generator's source
README is updated; its shipped README is regenerated with the assets by the
integrator, not edited by this worker.

## Tests written, not executed

- `backup::tests::checksum_conflict_cancel_retention_and_restore_round_trip`
- `backup::tests::independent_sha256_reference_and_mid_chunk_cancel`
- `remote_protocol::tests::{rfc6455_handshake_reference,masked_text_and_limits}`
- `remote_protocol::tests::http_duplicate_chunked_truncated_and_size_limits`
- `session::phone_remote::tests::authentication_replay_expiry_and_rate_limits`
- `session::phone_remote::tests::pairing_attempt_expiry_connection_and_action_shape_limits`
- `windfall_factory::loops::tests::{byte_stable_generation_and_metadata,tempo_fit_preserves_bars_and_key_fit_changes_pitch}`
- `windfall_factory::loops::tests::independent_wav_decode_and_pitch_measurement`
- `session::tests::remote_cloud_content::{remote_named_command_undo_stale_and_recording_guards,loopback_pair_websocket_fader_and_undo,backup_portable_project_restore_round_trip_and_original_generation,loop_fit_insertion_one_undo_save_reopen_and_provenance,rendered_loop_cannot_attach_to_replaced_or_recording_document}`
- `session::tests::remote_cloud_content::loop_drop_creates_blank_rows_and_clip_in_one_undo_step`
- Mounted UI `features/settings/phone-remote.test.tsx`:
  `starts only on explicit enable, shows pairing, and revokes on disable`;
  `shows native capability and bind errors without simulated success`;
  `reports a refused bind and keeps the listener disabled`.
- Mounted UI `features/backup/panel.test.tsx`:
  `chooses a folder and backs up only after explicit user action`;
  `reports history conflicts and allows cancellation of an active job`;
  `opt-in after-save backup carries the reviewed head and original generation`;
  `restores to a chosen new archive and uses the ordinary open flow`;
  `does not attach an automatic backup choice across project replacement`;
  `carries numbered-save configuration and honors a withdrawn opt-in`.
- Mounted UI `features/loops/browser.test.tsx`:
  `filters source tempos, auditions and produces a guarded fitted drop payload`;
  `inserts with native generation and revision as one backend operation`.

## Integrator commands (not run by this worker)

Run from the merged integration checkout, in Windows Git Bash. Reconcile the
Cargo.lock direct dependency edges without `--locked` before evaluating checks.
The output directory isolates generated bindings; review/copy them before UI
checks. Subsequent Rust tests export TS only into that temporary directory.

```sh
source scripts/msvc-env.sh
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$PWD/target/remote-cloud-content"
wf_bindings="$PWD/target/remote-cloud-content-bindings"
bash scripts/gen-bindings.sh "$wf_bindings"
export TS_RS_EXPORT_DIR="$(cygpath -w "$wf_bindings")"
cp "$wf_bindings"/* apps/desktop/src/bindings/
cargo run -p windfall-factory --release -- generate content/factory
cargo run -p windfall-factory --release -- verify content/factory
cargo test -p windfall-factory
cargo test -p windfall-desktop --lib backup
cargo test -p windfall-desktop --lib remote_protocol
cargo test -p windfall-desktop --lib phone_remote
cargo test -p windfall-desktop --lib session::tests::archive
cargo test -p windfall-desktop --lib session::tests::remote_cloud_content
cargo clippy -p windfall-factory -p windfall-ipc -p windfall-desktop --all-targets -- -D warnings
pnpm --dir apps/desktop test src/features/settings/phone-remote.test.tsx src/features/backup/panel.test.tsx src/features/loops/browser.test.tsx --maxWorkers=4
pnpm --dir apps/desktop test src/lib/flows/portable.test.ts src/lib/flows/project-generation.test.ts src/lib/actions/shortcuts.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Eight loop WAVs, `Loops/manifest.json` and the shipped factory README are
intentionally pending generation. The existing factory folder equality test
will fail until regeneration. Runtime browser/insertion also renders on demand.

Only `rustfmt --edition 2024` on the new Rust modules has run (success); this
is formatting, not compilation or acceptance evidence.

## Current row claims and remaining work

| Row | Claim | Remaining |
| --- | --- | --- |
| `core-fl-studio-remote` | Full-row software candidate, unverified | Integrator compilation/tests and real phone/LAN/firewall checks. HTTP is local plain text, with no TLS or Internet binding. |
| `fmt-cloud-backup` | Full-row software candidate, unverified; folder backup is the agreed product | Integrator compilation/tests, real synced-folder conflict/power-loss and restore checks. A cloud SDK/account adapter is optional future work, not a closure requirement. |
| `core-loop-starter` | Full-row software candidate, unverified | Factory asset regeneration, compilation/tests and listening/cross-platform byte checks. All v1 melodic loops are minor; fit is on insertion, not automatic after later project-tempo changes. |

Shared seams touched: native `commands.rs`, `lib.rs`, `session/mod.rs`,
`session/tests/mod.rs`, `session/archive.rs`, `session/transport.rs`;
`windfall-ipc/src/lib.rs`, factory `src/lib.rs`; frontend IPC
`backend.ts`, `tauri.ts`, `mock.ts`, browser `index.tsx`, playlist `grid.tsx`,
settings `settings-dialog.tsx`, and Save flows `lib/flows/{project,portable}.ts`.
No new project Command variants, descriptor indices, persisted Project fields,
DSP callback work or generated binding/WASM edits were introduced.

Generated types: `RemoteSettingsState`, `BackupVersion`, `LoopEntry`, `LoopCatalog`.
No generated files were edited. Cargo.lock needs normal integrator reconciliation
for direct reuse of already locked `sha2 = 0.10.9` and `getrandom = 0.3.4`.
No HTTP/server/cloud SDK dependency was added.
The user's product decision is explicit: provider-neutral folder backup closes
this row's software scope and works with any independently chosen synced-drive
client, without a Windfall cloud account. The real-provider adapter contract is
documented for future extensions; no adapter or SDK is required for this row.

## Known risks / external gates

Uncompiled Rust/TypeScript seams and strict lint first; bounded server wire and
shutdown/recording races next; then archive backup round trips and musical fit.
Source review corrected numeric TS exports for u64 IPC fields and loop undo
assertions to preserve retired IDs. Independent spectral fit thresholds, native
source-pool preparation/row allocation and the mounted UI harness are unexecuted
and deserve early inspection. Run existing Save/portable UI tests too because
the optional hook adds a native-identity request only when explicitly enabled.
Real phones/tablets, LAN/firewall behavior, synced-folder conflict/power-loss
behavior, cross-platform byte identity and listening review remain external.
Folder locks coordinate a single filesystem, not disconnected synced-drive
replicas. A cloud adapter must provide actual remote compare-and-publish.
Crash-stale lock recovery, synced-folder history conflicts and atomic history
durability after power loss require filesystem/provider-specific review. Backup
captures the current portable snapshot after Save, which may include concurrent
edits; it does not guarantee byte identity with the preceding Save file.
