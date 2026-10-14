# Automatic updates for Windfall 1.0.1 — 2026-10-14

Windfall 1.0.0 shipped with the updater code but nothing configured: no
public key and no endpoint, so every check ended in "Operator must
provide …". This round configures it for GitHub Releases, finishes the
behaviour, bumps the version to 1.0.1 and proves a real update from one
locally built version to a newer one.

- Main checkout, branch `feat/1.0.1` from `53b1c8fc` (the `v1.0.0` tag).
- One build or test process at a time through `target/autoupdate/run.sh`
  (MSVC environment, `CARGO_BUILD_JOBS=8`, Node and pnpm on PATH). Logs
  are `target/autoupdate-<step>.log`. No sub-agents.
- Nothing pushed, tagged or published. GitHub was only read: HEAD
  requests to count its redirects.
- The private key was used only by path, through `TAURI_SIGNING_PRIVATE_KEY`
  set from the file in the build script's own process. A scan of every
  tracked file and of the build logs and bundle folder finds none of it.

## 1. Configuration

**One source of truth: the `plugins.updater` block of
`apps/desktop/src-tauri/tauri.conf.json`.** The build-time variables
`WINDFALL_UPDATE_PUBLIC_KEY`, `WINDFALL_UPDATE_STABLE_URL` and
`WINDFALL_UPDATE_PREVIEW_URL` are gone.

| Field | Value |
|---|---|
| `pubkey` | `windfall-updater.key.pub`, base64 as Tauri stores it (key id `3FD1425647FF1A48`) |
| `endpoints[0]` (the stable channel) | `https://github.com/wheel338-sketch/windfall/releases/latest/download/latest.json` |
| `requireSignedVersion` | `true`: the signature must name the version the manifest announces |
| `windows.installMode` | `passive` (unchanged) |
| `dangerousInsecureTransportProtocol` | `true` (unchanged; the updater only ever talks to the app's own loopback replay of bytes it has already fetched) |

Why this file rather than the environment variables:

- the Tauri CLI reads the same `pubkey` when it signs. With a key
  configured, a bundle without the private key fails ("A public key has been
  found, but no private key"), so an unsigned release cannot slip out;
- a test build points elsewhere with `tauri build --config override.json`,
  Tauri's own merge, without editing the committed file;
- development builds need no key. `tauri dev` and `cargo build` do not
  bundle, and `tauri build --no-sign` (the CI candidate workflow's form)
  skips updater signing.

`updates.rs` reads the key and the first endpoint from the app's config at
run time (`configured_source`). The existing endpoint rules are kept:

- placeholder expansion;
- HTTPS, or literal loopback HTTP;
- no credentials;
- the 8 KiB length budget;
- HTTPS-only redirects, at most 3.

Release asset downloads take 2 redirects
(`releases/latest/download/…` → `releases/download/v…/…` →
`release-assets.githubusercontent.com`).

There is no preview channel. The Rust side reports it as not configured,
and the channel selector has been removed from Settings.

Other changes to the backend:

- The package URL in the manifest now follows the endpoint rule (HTTPS or
  literal loopback HTTP). Before, it had to be HTTPS, which would have made
  a loopback test host impossible. Every package is still verified against
  the embedded key.
- Fetch and download failures reach the window as the coded messages
  `updates.checkFailed` and `updates.downloadFailed`, translated in both
  catalogs. Before, they arrived as reqwest's raw text.
- The total timeout depends on the transfer: 60 s for the manifest and
  30 min for the installer, each with a 30 s read timeout. The installer had
  a flat 120 s, too short for 31 MiB on a slow line.
- `update_check` takes `background`. A debug build answers a background
  check with "Development builds do not check for updates by themselves.",
  so test runs of debug executables never reach GitHub. A manual check
  still works in a debug build.

## 2. Behaviour

- **Start-up check**: `startAutomaticChecks` in
  `features/updates/store.ts`, mounted once by `UpdateHost` in the main
  window. It is not inside the language boundary, so a language change
  cannot drop a pending decision.
  - It runs 8 s after the window is up, and only if a day has passed since
    the last check (`windfall.update-last-check`).
  - It is quiet: a failure is logged with `console.info` and shows nothing.
    It shows nothing when there is no update either.
- **Help → Check for updates…** (`help.checkUpdates`): the notice if an
  update is found. Otherwise a toast:
  - "You are up to date.";
  - for a failure, "Could not check for updates" with its cause.
- **The notice**: a non-modal card at the bottom left with:
  - "Windfall X is available";
  - the start of the release notes (Markdown marks removed, about 280
    characters at most);
  - download progress and errors;
  - **Install now** and **Later**.

  Install now downloads and verifies, then installs. The shell's existing
  shutdown path does the rest:
  - the unsaved-project prompt (save, discard or cancel);
  - audio and plugins quiesced;
  - rollback on cancel or on a refused installer;
  - the passive NSIS installer, which relaunches the app.

  Later hides the notice. The update stays in Settings and the Help menu.
- **Settings → Updates**:
  - a switch, *Check for updates automatically* (`windfall.update-auto-check`,
    on by default);
  - *Check for updates*;
  - the full notes, *Download and verify* and *Install and restart*, as
    before.
- English and Spanish strings are in `lib/i18n/messages/updates.ts` and the
  backend catalog. The three channel strings no longer used were removed.

## 3. Release tooling

- `scripts/release/update-manifest.mjs` writes `latest.json` in Tauri's
  static format:
  - `version`, `notes`, `pub_date`;
  - `platforms.windows-x86_64.{signature,url}`, with the GitHub release
    asset URL;
  - `windfall.{channel:"stable", projectFormatVersion}`, which the app's
    policy requires.

  It refuses a prerelease version. Before writing, it verifies the
  signature with Node's crypto, in full:
  - the key id against the embedded key;
  - the Ed25519 signature over the BLAKE2b-512 prehash;
  - the global signature over the trusted comment;
  - the signed `version` and `file`.

  Its tests (`update-manifest.test.mjs`, 5) use minisign fixtures made with
  Node alone. The verifier was also checked against a real
  `tauri signer sign --app-version` signature.
- `docs/RELEASE-ENGINEERING.md`, "Publishing an update": build with the
  key, run the script, upload in order, check.
- `docs/RELEASE-NOTES-1.0.1.md`: the user notes, which also go into
  `latest.json`.

## 4. Version

`1.0.0` → `1.0.1` in the places listed in the 1.0.0 QA record:

- the workspace `Cargo.toml` and the 16 workspace packages in `Cargo.lock`;
- `apps/desktop/package.json` and `tauri.conf.json`;
- the CLAP descriptor in `crates/windfall-plugin/src/clap.rs`.

The plugin runtime's `distribution.json` in the final bundle says `1.0.1`.

## 5. End-to-end proof

`scripts/auto-update-window-smoke.mjs OLD_SETUP NEW_SETUP WORK_DIR` uses the
window-smoke approach: WebView2 DevTools, real clicks, the software sink and
a profile of its own.

**The two builds.** Both are signed with the release key, built with
`target/autoupdate/build.sh e2e VERSION`, which runs
`tauri build --ci --bundles nsis --config <override>`. The override sets:

- `plugins.updater.endpoints` to `http://127.0.0.1:18765/latest.json`;
- `productName` to "Windfall Update Test" and `identifier` to
  `org.windfall.WindfallUpdateTest`;
- `version` to `1.0.1` and to `1.0.2`. 1.0.2 was never committed.

**Why a separate identity.** Windfall 1.0.0 is installed for this user in
`%LOCALAPPDATA%\Windfall`. The NSIS installer keys its registration,
install location and shortcuts on the product name. A test install under
"Windfall" would have overwritten that registration, and uninstalling it
would have removed it.

**Why 1.0.2 and not 1.0.2-test.** The stable channel refuses prerelease
versions by design (`policy`).

**The run:**

1. Silent per-user install of 1.0.1 into `target/autoupdate/e2e-run/install`
   (`/S /NS /D=…`, so no shortcuts).
2. Launch with `WINDFALL_SOFTWARE_AUDIO=1`, `WINDFALL_CONFIG_DIR`,
   `WEBVIEW2_USER_DATA_FOLDER` and a DevTools port.
3. A Node HTTP server on 127.0.0.1:18765 switches between manifests:
   - **tampered**: the genuine manifest, pointing at the installer with one
     byte flipped;
   - **wrong-key**: the genuine installer, signed for 1.0.2 with a key made
     for the run;
   - **replay**: the genuine installer and signature, announced as 1.0.3;
   - **missing**: 404, which is what GitHub answers today because 1.0.0
     has no `latest.json`;
   - **good**: written by `update-manifest.mjs` after it verified the real
     signature.

A probe beforehand showed that the NSIS plugin's `RunAsUser`, which
relaunches the app, passes on the installer's environment. The installer
gets the app's environment through `ShellExecute`. So the relaunched app
keeps the software sink and the test profile.

Two complete runs. Each passed 29 of 29 checks
(`target/autoupdate/e2e-run-report.json`, `e2e-run2-report.json`):

| Step | Run 1 | Run 2 |
|---|---|---|
| 1.0.1 installed silently into the test folder, registry `DisplayVersion` 1.0.1, exe ProductVersion 1.0.1 | pass | pass |
| Start-up to an interactive window; engine host `Software`, device "Memory sink (no audio device)" | 1,074 ms | 935 ms |
| No request before the start-up delay; the start-up check fetched `latest.json` by itself | after 8.9 s | after 8.8 s |
| **tampered** (start-up check): notice "Windfall 1.0.2 is available" → Install now → whole package downloaded → "Download or signature verification failed. Nothing was installed. The signature verification failed" | pass | pass |
| **wrong-key** (Help → Check for updates): refused, "… The signature was created with a different key than the one provided" | pass | pass |
| **replay**: notice for 1.0.3, refused, "… The update was signed for version 1.0.2 but the update endpoint announced version 1.0.3 …" | pass | pass |
| After each refusal: the 1.0.1 process alive, `plugin:app|version` 1.0.1, registry still 1.0.1, installed exe byte-identical, audio running again on the software sink, Later puts the notice away | pass | pass |
| **missing**: toast "Could not check for updates — Update check failed; retry when online. HTTP status client error (404 Not Found) …", no notice, app alive | pass | pass |
| **good**: notice with the version and the notes summary; Install now; whole installer downloaded once (32,511,003 bytes); 1.0.1 exits with code 0, without an unsaved prompt (the new default project is clean) | exit 725 ms after the click | 724 ms |
| The passive installer relaunches `install\windfall-desktop.exe` as 1.0.2: `plugin:app|version` 1.0.2, registry 1.0.2, exe ProductVersion 1.0.2, executable replaced in place | 3.3 s after the exit | 3.3 s |
| The relaunched app is on the software sink with the test profile; Help → Check for updates says "You are up to date." | pass | pass |
| Closing the 1.0.2 window quits it; no uncaught exception in either session | pass | pass |
| Cleanup: uninstalled (registry keys gone, exe gone), the test location key and app-data folders deleted, updater temp folders deleted, no shortcuts; the user's own `Uninstall\Windfall` and `Software\windfall\Windfall` keys and `%LOCALAPPDATA%\Windfall\windfall-desktop.exe` unchanged | pass | pass |

An earlier attempt stopped before installing anything. The script gave the
foreign-key signer an empty `TAURI_SIGNING_PRIVATE_KEY`, which the CLI
counts as a second key. It now removes those variables. Cleanup was
complete that time too.

Afterwards the test installers, the override configs and the test-identity
bundles were deleted. No "Windfall Update Test" registration, app-data
folder or temp folder remains.

## 6. Gates

Run one at a time by `target/autoupdate/gates.sh` on `710fe3d3`. Rust
counts are passed / failed / ignored.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check` | pass |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| Native binaries and fixtures | `cargo build -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib` | built |
| Exported plugin (changed: `clap.rs`) | `cargo test -p windfall-plugin --no-fail-fast -- --test-threads=1` | 37 / 0 / 0 |
| Desktop (changed: `updates.rs`, `update_transfer.rs`), including the message guards | `cargo test -p windfall-desktop --no-fail-fast -- --test-threads=1` | 770 / 0 / 14 |
| Frontend typecheck / lint | `pnpm --dir apps/desktop typecheck`, `lint` | pass, pass |
| Full frontend suite | `vitest run --maxWorkers=4` | 540 files, 6,535 / 0 |
| Release scripts | `node --test scripts/release/{release,workflow,update-manifest}.test.mjs` | 74 / 0 |

New and changed tests:

- `updates.rs`:
  - signatures bound to the announced version are accepted; unbound and
    other-version signatures are refused, like a wrong key, corruption or
    truncation;
  - the committed configuration has the key, the one GitHub endpoint, no
    preview and `requireSignedVersion`;
  - loopback HTTP package URLs are accepted and other HTTP is refused.
- `features/updates/notice.test.tsx`, 15 tests:
  - due-once-a-day scheduling, including a clock set back;
  - the delay;
  - the setting and its persistence;
  - no check outside the desktop app;
  - quiet background failures;
  - the notice: summary, Later, Install now through download, verify and
    install;
  - the unsaved prompt and cancel;
  - a refused package and retry;
  - installer refusal and rollback;
  - the Help menu entry, its up-to-date toast and its failure toast.
- `section.test.tsx`: its 6 tests now drive the shared store. Their
  assertions are unchanged.

## 7. The release artifacts

| | |
|---|---|
| Installer | `target/release/bundle/nsis/Windfall_1.0.1_x64-setup.exe` |
| Size | 32,609,104 bytes (31.10 MiB) |
| SHA-256 | `ab3a9c54b8c8f4551e687f4a9f9389ddd073ba06fad1b1e122ecb8333d805ec1` |
| Signature | `target/release/bundle/nsis/Windfall_1.0.1_x64-setup.exe.sig`, SHA-256 `c813122426022b1eaa795c51fad36f384e1417a262bd2f750c96c7aafb483cc6`, trusted comment `file:Windfall_1.0.1_x64-setup.exe`, `version:1.0.1` |
| Manifest | `target/release/bundle/nsis/latest.json`, SHA-256 `fe74bbed965498982b9511d90050aa85b77c0acedc8b4b046c9a003a4c329d5f`; `url` `https://github.com/wheel338-sketch/windfall/releases/download/v1.0.1/Windfall_1.0.1_x64-setup.exe`, `pub_date` `2026-10-14T15:50:42Z` |
| Built from | `710fe3d3` (the commit after it adds only this record) |
| Main executable | `windfall-desktop.exe`, SHA-256 `4027e729fc3bce8a16a084439ce7fa3ee70088865c8298b7ecf9c7ab3d195f17`, ProductVersion 1.0.1 |
| Bundle | 82 `File /a` resources, as in 1.0.0; plugin runtime `distribution.json` 1.0.1 |
| OS code signing | none (updater signature only); SmartScreen still warns |

To publish:

1. Create `v1.0.1` as a draft release.
2. Upload, in this order:
   1. `Windfall_1.0.1_x64-setup.exe`;
   2. `Windfall_1.0.1_x64-setup.exe.sig`;
   3. `latest.json`.
3. Publish it as the latest release, not a prerelease.

Check that `releases/latest/download/latest.json` then returns version
1.0.1.

## 8. Unresolved

- **1.0.0 users must install 1.0.1 by hand once.** 1.0.0 has no key or
  endpoint and cannot be updated. From 1.0.1 on, updates work.
- The private key has no password and exists only at
  `C:/Users/ewhee/.windfall-release/`. If it is lost, installed versions
  cannot be updated again without a manual install. It needs a backup kept
  somewhere else, outside the repository.
- The end-to-end update ran on builds that differed from the release only
  by the `--config` override (endpoint and product identity). The release
  binary itself was not installed, because that would have touched the
  user's own Windfall registration. Its configuration is covered by the
  unit test of the committed config. The live GitHub path (2 redirects to
  `release-assets.githubusercontent.com`) has only been checked with HEAD
  requests until 1.0.1 is published.
- The updater leaves its downloaded installer in a
  `%TEMP%\Windfall-VERSION-updater-*` folder (upstream behaviour).
- The update was proved for Windows x64 NSIS only. macOS and Linux have no
  manifest entries.
- The unsaved-project prompt was not shown in the end-to-end run, because
  the new project was clean. It is covered by the UI tests and the existing
  native shutdown tests.
