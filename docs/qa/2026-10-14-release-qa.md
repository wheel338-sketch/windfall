# Release QA for Windfall 1.0.0 — 2026-10-14

This is a quick check of the real Windows release before the public
`v1.0.0` release: the version bumped, the release installer built, and the
release executable driven through its window like a user making a song.

- Main checkout, branch `feat/parity-100`, started at `3ee338d0` (clean).
- One build or test process at a time, through `target/release-qa/run.sh`
  (MSVC environment, `CARGO_BUILD_JOBS=8`, Node and pnpm on PATH). Logs are
  `target/release-qa-<step>.log`. No sub-agents.
- No sound on an audible device, no microphone, no MIDI hardware: every run
  of the app used the software memory sink.
- The installer was not run. Its contents were read from the NSIS script
  Tauri generated, and the release executable was run from a copy laid out
  the way the installer lays it out.
- Nothing pushed, tagged or published.

## 1. Version

`0.1.0-alpha.1` → `1.0.0` in:

- the workspace `Cargo.toml` (every crate takes `version.workspace`) and
  `Cargo.lock`;
- `apps/desktop/package.json` and `apps/desktop/src-tauri/tauri.conf.json`;
- the exported CLAP plugin's descriptor (`crates/windfall-plugin/src/clap.rs`,
  a fixed C string). The VST3 side and the plugin runtime manifest already
  read `CARGO_PKG_VERSION`.

Help → About said "Windfall 0.1.0" from a literal. It now reads the version
from `package.json` (`resolveJsonModule` in `tsconfig.app.json`). The plugin
runtime manifest (`scripts/build-plugin-runtime.mjs`) reads the workspace
version and wrote `1.0.0`.

Left alone on purpose:

- the published alpha's immutable contract in
  `scripts/release/contracts.mjs` (`ALPHA`);
- the updater policy test's sample versions in `updates.rs`;
- historical QA logs.

The old alpha tag and assets were not touched; the old installer is still
in `target/release/bundle/nsis/`. Commit `ec779fca`.

## 2. Build

The build command was `pnpm --dir apps/desktop tauri build --ci --no-sign
--bundles nsis`, the release workflow's own form. `targets: "all"` would
also try MSI, which the alpha never shipped. The plugin runtime was
prebuilt with 8 jobs first, because the `beforeBuildCommand` builds it with
`--jobs 1`; that step is then a no-op.

| Build | Wall time | Notes |
|---|---|---|
| First, after the version bump (`ec779fca` + the sink change) | 1,290 s | plugin runtime library 102 s, audio helper 25 s, `tauri build` step 1,158 s (desktop crate release compile 8 min 0 s; every workspace crate rebuilt because the version changed). Installer 32,505,496 bytes, SHA-256 `ab2f9e7f…`, superseded |
| Rebuilds after the layout fixes | 326 s, 228 s | frontend and desktop crate only |
| **Final, from `c347d7a4`** | 378 s | desktop crate 4 min 57 s; installer `Windfall_1.0.0_x64-setup.exe`, 32,648,429 bytes (31.14 MiB) |

The final installer's file list was compared again: 82 `File /a` entries
(44 `factory`, 35 `examples`, 3 `plugin-runtime`), the same as below, and
`plugin-runtime/distribution.json` says `1.0.0`.

Bundle contents, from `target/release/nsis/x64/installer.nsi` (82 `File /a`
entries plus the main binary), compared with the sources:

| Resource | In the installer | Source |
|---|---|---|
| `windfall-desktop.exe` (also the plugin-bridge helper, run as itself) | yes | |
| `factory/` | 44 files: every kit, bass and loop WAV, `manifest.json`, `LICENSE.md`, `README.md` | `content/factory` has 44 files |
| `examples/effects/` | 35 files | `docs/examples/dynamics-eq` has 35 |
| `plugin-runtime/` | `windfall_plugin.dll`, `windfall-plugin-audio.exe`, `distribution.json` (`abi 1`, `version 1.0.0`, both SHA-256 listed) | `target/release/plugin-runtime` |
| WebView2 | bootstrapper, downloaded on install if missing (Tauri default) | |

The assistant's help text is compiled in (`include_str!`). Nothing the app
loads at run time is missing.

The installer is unsigned and the updater artifacts are not signed
(`--no-sign`). No updater endpoint is configured.

## 3. The song test

The test is `scripts/song-workflow-window-smoke.mjs EXE REPORT [PORT]`, a new
window smoke built on the pattern of the existing ones:

- it launches the app with `WINDFALL_SOFTWARE_AUDIO=1`;
- it gives the app a profile folder of its own through `WINDFALL_CONFIG_DIR`
  and `WEBVIEW2_USER_DATA_FOLDER`;
- it attaches through WebView2 DevTools on a local port;
- it answers the system file dialogs through UI Automation and window
  messages.

It ran on `target/release-qa/stage/windfall-desktop.exe`: the release
executable with `factory/`, `examples/` and `plugin-runtime/` copied beside
it, as the installer places them. The app's log confirmed that it read the
factory content from that folder, not from the source tree.

The steps below run in one session and then a second launch. Everything is
a click, a key or a file dialog except where it says "by command".

1. **Start-up.** Launch, attach, and wait until the channel rack is drawn
   and the backend answers. Timed from process start.
2. **New project.** File → New project: the default template, with the
   factory kit (Kick Punch, Clap Wide, Hat Closed 1, Snare Tight), one mixer
   track each, and a clean history.
3. **Drum pattern.** Twelve step buttons clicked in the channel rack (kick
   1/5/9/13, clap 5/13, hat offbeats, snare 8/16). Each lane is checked step
   by step in the document.
4. **Instrument and melody.**
   - Add channel → Add subtractive synth; Patterns → Add pattern.
   - The synth's channel button, then the Piano roll tab.
   - One probe note with Snap set to None measures ticks per pixel. It is
     removed with Ctrl+Z.
   - Snap = Beat, and seven notes are drawn over two bars (C E G F, then
     E D B). Starts and keys are checked against what was drawn.
5. **Arrangement.** Playlist tab, Snap stepped to Bar with Coarser. The
   drum pattern is clicked onto bars 1 to 4 of track 1, and the two-bar
   melody onto bars 1 and 3 of track 2. Clicks are 150 ms apart, a person's
   pace. Clip starts and lengths are checked.
6. **Mixer.**
   - The clap's routing badge → Route to a new mixer track.
   - The snare's badge → that new track, so it is a bus for both.
   - The strips' Add effect menus: Delay on the synth's track, Reverb on
     the bus.
7. **Automation.** Right-click the synth's volume knob → Create automation
   clip; the clip and its track appear on the playlist. The curve's three
   points are set **by command** (`setAutomationPoints`), because drawing
   points in the clip is a gesture on the canvas. Then Compressor on the
   kick's track.
8. **Undo and redo.** Ctrl+Z for each of the last three history steps
   (automation clip, curve, compressor). The project must equal the
   snapshot taken before them, except the id counter, which by design never
   goes back. Then Ctrl+Y as many times: the project must equal the
   snapshot taken after them.
9. **Play and stop.** Song mode from the transport, then the Play button.
   After 2.5 s:
   - the song position readout has moved (for example `001:01:1` →
     `002:02:1`, 2.52 s);
   - the engine's meter frames show a master peak of about 0.29;
   - the master meter canvas draws signal.

   Stop stops the transport. The transport layout is checked again while
   the readouts show figures.
10. **Browser.** Library tab, Factory → Bass expanded, then double-click
    `Bass Sub.wav`: a new channel plays the factory sample.
11. **Settings and language.** Options → Settings…, Interface language →
    Español: the menus are Spanish and no panel crashed. Then Opciones →
    Ajustes… → English.
12. **Export.** File → Export audio…, Render: The whole song, Choose… →
    the system Save dialog → `Song QA.wav`, Export. The script decodes the
    file itself and checks:
    - the format;
    - the length against the arrangement: four bars at 120 bpm is 8.000 s,
      plus the tail the dialog keeps until it has faded;
    - RMS and peak, and that there is no NaN or infinity;
    - that there is sound from the first frame to the last bar.
13. **Save.** File → Save as… → system dialog → `Song QA.windfall`. The
    window must report the project saved, and the file on disk must have
    the project's channels and clips.
14. **Quit.** The main window is closed with `WM_CLOSE`, as the close box
    does. The process must exit with code 0 within 20 s.
15. **Reopen.** A second launch with the same profile; File → Open project…
    → system dialog → the saved file.
    - The project must equal the saved one exactly (notes, clips, mixer with
      routing and effects, automation) and be clean.
    - The song is exported again through the dialog and must be the same
      audio, sample for sample.
    - The second window is closed the same way.
16. **Console.** Errors, asserts and warnings from
    `Runtime.consoleAPICalled`, `Runtime.exceptionThrown`, and `Log.entryAdded`
    errors are collected from both sessions. Any exception or error fails the
    run. Steps also check for a crashed panel (the panel error boundary's
    alert).

### Results

The final runs were on the release build from `1a5e8cdb`. The frontend and
native code are as at that commit; later commits change only the script's
documentation and the docs.

| | Run 1 (`target/release-qa/song-run2`) | Run 2 (`song-run3`) |
|---|---|---|
| Checks | 46 / 46 | 46 / 46 |
| Start-up to an interactive window, first launch | 1,094 ms (first start of a new build, cold file cache) | 636 ms |
| Start-up, second launch | 704 ms | 687 ms |
| Export render (Export pressed → file written) | 270 ms; reopened song 274 ms | 277 ms; 263 ms |
| Export step including the dialogs | 18.4 s | 18.3 s |
| Closing the window → process exit | 301 ms and 245 ms (exit 0, 0) | 233 ms and 228 ms (exit 0, 0) |
| Exported WAV | 24-bit PCM, 48 kHz, stereo, 474,085 frames = 9.877 s (8.000 s song + 1.877 s delay and reverb tail); peak 0.4165, RMS 0.0610; no NaN; no sample at or over full scale | identical |
| Reopened project | equal to the saved project; re-export identical to the first export | same |
| Webview console | no exception, no error | same |

Almost all of the export step's 18 s is the test's own file dialog
automation (one PowerShell UI Automation process per dialog). The render
itself takes about a quarter of a second. The two runs' exports are
byte-identical (`cmp` on the sample data). Each run takes about 52 s.

Not exercised: real audio and MIDI devices (by the rules of this check),
third-party plugins, recording, and the installer itself, including
uninstall and upgrade.

## 4. Bugs found and fixed

Each was found by this run, fixed in the product, rebuilt and re-run.

1. **The shipped executable could not be checked without real devices**
   (`f6523411`). `WINDFALL_SOFTWARE_AUDIO`, `WINDFALL_CONFIG_DIR` and
   `WINDFALL_OUTPUT_GAIN` were honoured only by debug builds (`shell.rs`,
   and `Engine::software` behind `cfg(debug_assertions)`). The release
   executable would therefore have opened the default output device and
   MIDI hardware and used the user's own settings folder.
   - They now work in every build. They can only make the app quieter or
     more isolated.
   - The analyzer smoke hook stays debug-only.
   - The panel layout file now follows the config folder too; it had
     ignored the override in every build.
2. **The transport bar did not fit the default 1440×900 window**
   (`1a5e8cdb`). Its content was 1,493 px wide:
   - Redo was past the right edge.
   - The master meter and the CPU, RAM and dropout readouts sat in a
     `min-w-0 flex-1` box squeezed to zero width, so they were painted over
     the pattern selector. The meter's clip-indicator button and the CPU
     text lay on top of it, so at the default size **the pattern selector
     could not be clicked at all**.

   My first fix clipped the readouts and let the selector shrink. That hid
   the master meter entirely, and the smoke's meter check caught it. The
   fix kept lets the bar wrap onto a second row when the window is too
   narrow; the meter and readouts move down together (`min-w-72`).
3. **The browser's second row of tabs covered its search box**
   (`1a5e8cdb`). The tab list's `h-auto` lost to the base style's
   `group-data-horizontal/tabs:h-8`. Once that was overridden, the tabs
   (`h-[calc(100%-1px)]`) stretched to the list, 43 px each. The list now
   grows and its tabs are 24 px tall.
4. **Help → About said "Windfall 0.1.0"** from a literal (`ec779fca`). It
   now reads the package version.

What the run looked at that is not a bug:

- The playlist ignores a press while it is still committing the previous
  placement (`session.ts`: `pointerDown` returns unless the gesture is
  idle). Clicks 10 ms apart lost all but the first; at 150 ms every click
  was taken. A person does not click that fast, so this is left as it is.
- Undo does not take the project's id counter back; ids are never reused.
- The "dropouts" figure rises on the memory sink, because its timer thread
  wakes late under Windows' timer resolution. It says nothing about real
  devices.

## 5. Gates after the fixes

Run one at a time by `target/release-qa/gates.sh` on `c347d7a4`. Rust
counts are passed / failed / ignored.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all --check` | pass |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass, no diagnostics |
| Native binaries and fixtures | `cargo build -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib` | built |
| Engine (changed: `device.rs`) | `cargo test -p windfall-engine --no-fail-fast -- --test-threads=1` | 936 / 0 / 3 |
| Exported plugin (changed: `clap.rs`) | `cargo test -p windfall-plugin --no-fail-fast -- --test-threads=1` | 37 / 0 / 0 |
| Desktop (changed: `shell.rs`, `lib.rs`), with the plugin fixture variables | `cargo test -p windfall-desktop --no-fail-fast -- --test-threads=1` | 769 / 0 / 14 |
| Frontend typecheck | `pnpm --dir apps/desktop typecheck` | pass |
| Frontend lint | `pnpm --dir apps/desktop lint` | pass |
| Frontend tests of the changed areas | `vitest run --maxWorkers=4 src/features/transport src/features/browser src/lib/actions src/features/layout` | 54 files, 555 / 0 |
| Release scripts | `node --test scripts/release/release.test.mjs scripts/release/workflow.test.mjs` | 69 / 0 |

The full frontend suite was not run: the changes are two class lists, the
About toast's text and a compiler option, and the typecheck covers the
last. The full Rust workspace suite was not rerun either; only the three
crates changed. Both were green at `3ee338d0`.

The song smoke on the final executable (staged as before) ran three times:

| Run | Result |
|---|---|
| `target/release-qa/song-final` | 42 checks passed, then failed: the second session's system Save dialog for the re-export was not found within 15 s (`picker` exit 3). Everything before it, including the exact comparison of the reopened project, had passed. |
| `song-final2` | 46 / 46; start-up 620 ms and 612 ms; export render 0.26 s; exits 0, 0 |
| `song-final3` | 46 / 46; start-up 612 ms and 653 ms; exits 0, 0 |

Of the five runs on the fixed builds that reached that step, it failed once.
What was recorded does not show whether the app's dialog opened late or
the test's PowerShell UI Automation process was slow to find it; it is
listed under "Known and not fixed". The exported audio of the final
build is byte-identical to the earlier runs'.

## 6. Known and not fixed

- **One intermittent failure of the song smoke** (1 of 5 runs that reached it): the system
  Save dialog for the second export was not found within 15 s. The cause
  is not known; the other four runs passed every check.
- **The installer is unsigned**, so SmartScreen will warn. The updater
  artifacts are unsigned and no update endpoint is configured.
- **No device testing** in this check (memory sink only), no third-party
  plugins, no recording. The installer was not run; its file list was
  checked instead.
- At the default window size the transport bar now takes two rows. In a
  window about 1,800 px or wider it takes one. A more compact single-row
  design is for later.
- The piano roll's toolbar is wider than the window at 1440 px and scrolls
  horizontally with its own scrollbar; nothing is hidden for good.
- `README.md` still describes the project as early and links the alpha in
  a private repository, which is out of date for a 1.0 release. It was not
  changed here; the release notes are in `docs/RELEASE-NOTES-1.0.0.md`.
- As the release notes say, some Spanish text is still English and the
  Spanish has not had a native speaker's review (see the languages record).

## 7. The installer to publish

| | |
|---|---|
| File | `target/release/bundle/nsis/Windfall_1.0.0_x64-setup.exe` |
| Size | 32,648,429 bytes (31.14 MiB) |
| SHA-256 | `e28d01d2aa37e09af9a08346647c56b0779331d934693cfef29826bbe483f578` |
| Built from | `c347d7a4` (the commit after it adds only this record and the release notes, so the sources the binary is built from are the same) |
| Main executable inside | `windfall-desktop.exe`, SHA-256 `1cb70ba572a2c408f3ff64e5dac2b524cc67d403c84b39a38e86abe06b4c07ca` |
| Signing | none (`--no-sign`) |

Release notes for users: `docs/RELEASE-NOTES-1.0.0.md`.
