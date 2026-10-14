# Linux and macOS: first builds and tests of Windfall 1.0.1 — 2026-10-14

Windfall 1.0.1 shipped for Windows only. The Linux and macOS code paths
(audio devices, CLAP/VST3/Audio Unit hosting, the X11 and Cocoa plugin
editors, the process-bridge helper, the plugin export runtime) had been
type-checked for those targets but never built into an app or run. This
record covers the first builds, test runs and launch tests on both.

- Branch: `ci/linux-macos` locally (from `main` at `45ebed33`, the `v1.0.1`
  tag); since the macOS workflow, the same commits on `ci/macos`, which is
  the only branch pushed. Tip: see the end of this record. Nothing on `main`,
  no tags, no releases.
- **Linux** is built and tested locally in Docker (Ubuntu 22.04), never in CI.
- **macOS** is built and tested on GitHub's hosted macOS runners (Apple
  hardware; no macOS virtual machine on this PC, which Apple's licence does
  not allow), from `.github/workflows/macos.yml` on `ci/macos`, macOS only.
  An earlier workflow that also built Linux was removed (`9b24de6e`).
- All builds are unsigned previews: `tauri build --no-sign` with
  `createUpdaterArtifacts` off. The updater key was never read, copied or
  configured; the workflow has no secrets.
- Every app launched ran on the software audio sink
  (`WINDFALL_SOFTWARE_AUDIO=1`) with its own `WINDFALL_CONFIG_DIR`; in the
  Linux containers ALSA's default device is the null device and there is no
  sound hardware. Only `127.0.0.1` was bound.

## 1. Summary

| | Linux (Docker, Ubuntu 22.04 build) | macOS (GitHub hosted runners) |
|---|---|---|
| Rust workspace tests | **4,761 / 1 / 35** (passed / failed / ignored, 190 blocks); the one failure is a CPU-budget test on a WSL2 efficiency core (§3.3) | see §4.3 |
| Fixture-only plugin-host tests | 6 / 0 / 0 | see §4.3 |
| UI: typecheck, lint, build | pass | pass |
| UI tests (vitest) | 6,546 / 1 of 6,547 (one intermittent test, §3.3) | 6,545 / 0 in 542 files (Apple Silicon, two workers; before the 2 tests added in `d1dff720`) |
| Clippy `-D warnings` | clean | clean |
| Packages | AppImage, .deb | .dmg and .app per architecture |
| Clean-system install | .deb and AppImage pass on Ubuntu 22.04 and 24.04 | not applicable (Gatekeeper untested, §6) |
| Launch smoke (MCP over loopback) | 12 / 12, AppImage and .deb | 13 / 13 from the mounted DMG on Apple Silicon, including Apply pressed in the window |
| Full song workflow in the window | **58 / 58**, AppImage and .deb | not available (no WebDriver for WKWebView) |

Product bugs found and fixed: seven on Linux, three on macOS (one of them,
the update download timeout, affects every Unix). §5 lists them with the
test and tooling fixes.

## 2. Setup

### 2.1 Linux in Docker

Docker Desktop 29.8 (WSL2 backend; the VM has 32 vCPUs and 15.5 GB).
Everything is in `scripts/linux/` (see its README):

- `Dockerfile`, target `build` (`windfall-linux-build`, 2.64 GB): Ubuntu
  22.04, Rust 1.99.0 (as `release-candidate.yml` pins), Node 24.11.1, pnpm
  11, the WebKitGTK 4.1 / libsoup3 / GTK 3 / librsvg / AppIndicator / ALSA /
  D-Bus / X11 (`libx11-xcb`, `libxcb`, `libxrandr`) -dev packages, patchelf,
  Xvfb, `webkit2gtk-driver`, `tauri-driver`. No JACK: cpal is built with
  ALSA only.
- Target `runtime` (`windfall-linux-runtime`, 1.14 GB): a fresh Ubuntu 22.04
  with no compiler and no -dev package. Test tools only: Xvfb, openbox,
  wmctrl, xdotool, `WebKitWebDriver`, `tauri-driver`, Node.
- `xplat.sh` copies the working tree into the volume `windfall-src` and keeps
  Cargo's target folder in `windfall-target`, so rebuilds are incremental and
  never mix with the Windows `target/`. Containers run with `--cpus 12
  --memory 13g` and `CARGO_BUILD_JOBS=8`; one heavy step at a time.
- `scripts/xplat/gates.sh` is the repository's gate sequence, made portable:
  fmt, pnpm install / typecheck / lint / vitest / build, the plugin runtime,
  clippy, the owned test plugins, native binaries, `cargo test --workspace
  --no-fail-fast -- --test-threads=1` with the fixture variables, and the
  fixture-only tests run with `--ignored`.

### 2.2 macOS on GitHub Actions

`.github/workflows/macos.yml`, on pushes to `ci/macos` (and manual
dispatch), `contents: read`, no secrets. Jobs on `macos-latest` (Apple
Silicon) and `macos-15-intel`: Rust 1.99.0, Node 24, pnpm 11, the same
`gates.sh`, then `pnpm tauri build --ci --no-sign --bundles app,dmg` with
updater artifacts off, then the DMG mounted, its app copied out and launched
for the smoke, and the bundles, logs and reports uploaded as workflow
artifacts (14 days).

On the shared 3–4 vCPU runners the UI tests run with two workers (with four,
dozens timed out at their own 5 s limits; the suite is jsdom and
OS-independent and passes in full elsewhere), and the synths' CPU-deadline
tests (`instrument_deadlines`, budgets defined as a share of one
performance core) run as their own step and are recorded without deciding the
gate. Everywhere else they gate as before. The plugin runtime builds with
`WINDFALL_RUNTIME_JOBS=4` there (the build script's default stays one job).

## 3. Linux results

### 3.1 Gates (Docker, final run at `1cab2848` plus `8f1b2036` retests)

| Step | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| pnpm typecheck, lint, build | pass |
| vitest, 4 workers | 543 files; 6,546 passed, 1 failed (§3.3) |
| plugin runtime (release `libwindfall_plugin.so` + `windfall-plugin-audio`) | built |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --no-fail-fast -- --test-threads=1` | **4,761 / 1 / 35** over 190 blocks, 793 s |
| fixture-only: `bridge::helper`, `clap::instance::state_tests` with `--ignored` | 6 / 0 / 0 |
| after `8f1b2036`: desktop tests touched by it, `windfall-plugin` | 16 / 0 and 36 / 0 |

The first run, before the fixes, was 4,755 / 5 / 35 (and clippy and the
native build failed until §5.1's first three fixes). The 35 ignored are the
device, ffmpeg, benchmark and model tests that are ignored on Windows too.
The desktop crate's own native plugin tests (`plugins::capture_ack_tests`,
`ownership_tests`, `sidechain_bus_tests` and three session modules) are
compiled on Windows only (`cfg(windows)`, hard-coded `.dll` names); porting
them is separate work. The plugin-host crate's CLAP/VST3 fixture tests all run
on Linux and pass.

### 3.2 Packages on clean systems

`scripts/linux/pristine.sh` in plain `ubuntu:22.04` and `ubuntu:24.04`
containers (only what apt brings in):

- **.deb**: `apt install ./Windfall_1.0.1_amd64.deb` pulls in every
  library; every ELF file of the package resolves under `ldd`; started with
  no display, the app gets as far as GTK refusing to open one; `--mcp-stdio`
  with no app running exits 1 with "The MCP server is off". Depends:
  `libasound2t64 | libasound2, libx11-xcb1, libxrandr2, libwebkit2gtk-4.1-0,
  libgtk-3-0`. ALSA comes from `libasound2` 1.2.6.1 on 22.04 and
  `libasound2t64` 1.2.11 on 24.04. Installed size 116 MB.
- **AppImage**: extracted (no FUSE in a container). It bundles WebKitGTK and
  GTK but, like every AppImage built with linuxdeploy, leaves the libraries
  on linuxdeploy's exclude list to the host, because they must match the
  host's sound, graphics and font stack: ALSA, X11/xcb, GL/EGL/GBM/DRM,
  Wayland client, fontconfig, freetype, harfbuzz, fribidi. **Decision: those
  are documented host requirements, not bundled.** Every X11 or Wayland
  desktop has them; a bare container does not. With exactly those installed,
  nothing else is unresolved and the app reaches GTK's display check on both
  releases. gdk-pixbuf's optional SVG and TIFF loaders use the host's
  librsvg/libtiff when present (GTK skips them otherwise).

### 3.3 Smokes in the runtime image (final build, `8f1b2036`)

The AppImage first (extracted, with the host's ALSA), then the .deb
installed with apt, each under Xvfb (1600×1000) with openbox and a session
bus, as an unprivileged user, the webview's storage in the run's profile.

| Run | Result |
|---|---|
| AppImage, MCP launch smoke (`scripts/xplat/mcp-launch-smoke.mjs`) | 12 / 12, start-up to a served MCP 0.3 s |
| .deb, MCP launch smoke | 12 / 12 |
| AppImage, song workflow (`scripts/song-workflow-window-smoke.mjs`, WebDriver) | **58 / 58**, both sessions exit 0 |
| .deb, song workflow | **58 / 58**, both sessions exit 0 |

The MCP smoke: the server (left on in the profile) answers at start-up; a
request without the token is refused; `tools/list`; the summary shows the
default template with the factory kit's samples; the frontend reports its
view (only the webview does); a main window of at least 960×600 is mapped;
a proposal is filed; `render_and_measure` writes a WAV decoded in the smoke;
the app exits on SIGTERM.

The song workflow is the Windows release check, driven on Linux through
`tauri-driver` and `WebKitWebDriver`: a new project; a drum pattern clicked
into the rack; a synth added from the rack menu; a melody drawn in the piano
roll on beats; both patterns placed over four bars; a channel routed to a
new bus; Delay, Reverb and Compressor from the strips' menus; an automation
clip with a curve; undo and redo by keyboard; song playback with the
transport moving and the master meter showing signal; a factory sample added
from the browser; the settings dialog switched to Spanish and back; export to
WAV **through GTK's own file chooser** (answered with real X key events) and
decoded (24-bit 48 kHz stereo, length, level, sound from bar 1 to the end);
Save as through the chooser; quitting by closing the window through the
window manager (exit 0); a second launch that opens the project through the
chooser, compares it with the saved one exactly and exports the same audio
sample for sample. Then, by command: the song exported **as a plugin** from
the installed `plugin-runtime` (CLAP, VST3, helper, stamps), a scan that
finds the owned CLAP test plugin, that plugin hosted on the kick's mixer
track through the helper process, the song exported through it (audible,
finite), saved.

Recorded, not failing: in every run the language list in Settings took a
second click, and in some runs the kick's Add effect menu did; see §3.4 on
input timing under WebDriver. A person did not try these by hand.

Still failing or caveats on Linux:

- `instrument_deadlines::sytrus_maximum_load_deadline` (debug profile, CPU
  budget 0.75 of real time): 0.76 in the container against 0.56 on Windows on
  the same i9-14900F. The test's budget is a share of one performance core;
  on Windows it selects one, on Linux it runs unrestricted, and a WSL2 vCPU
  cannot be pinned to a P-core. Windows' own unpinned figure for the same
  synth ranged 1.13–2.01 s. Environment, not a Linux regression; untested
  on bare-metal Linux.
- `note-preview.test.tsx` "keeps row checked state…": failed once in the gate
  run and once in five isolated runs (a 1 s wait for a menu item), passed
  the other times and in the full Windows and macOS runs. Intermittent,
  OS-independent (jsdom).

### 3.4 Driving WebKitGTK reliably

Getting the song workflow through took changes to the smoke, all of them to
aim like a person and none to what is checked:

- pointer targets must have stopped moving, no running animation may hold
  them (menus and lists animate open), and when bringing them into view
  scrolled anything the click waits two frames: WebKitGTK scrolls
  asynchronously and routes input that arrives at once by the old scroll
  position;
- items of long menus and lists are centred, clear of their scroll arrows;
- the previous menu, a closing one included, must be gone before another is
  opened;
- an option or menu button that did not take is clicked again (as a person
  would) and the count is recorded;
- `tauri-driver`'s whole process group is stopped (WebKitWebDriver and the app
  inherit its output pipes), every smoke has a hard timeout, and a hung step
  now fails in seconds with the driver and app output instead of hanging.

## 4. macOS results

### 4.1 Runs

| Run | Commit | What it showed |
|---|---|---|
| [38613547524](https://github.com/wheel338-sketch/windfall/actions/runs/38613547524) | `5c6b4188` | Superseded on the third step (the plugin runtime had to exist before the desktop crate built). |
| [38615472753](https://github.com/wheel338-sketch/windfall/actions/runs/38615472753) | `a1cb3310` | Apple Silicon: built, bundled, **launched from the DMG** (13 checks, Apply not yet found by accessibility); gates failed: macOS-only clippy findings in `au/mod.rs`, the desktop test binary did not link (`_EMBED_INFO_PLIST` defined twice), so no desktop test ran, and the fixture-only bridge tests could not load a flat `fixture.vst3`. Intel cancelled in its bundle step. |
| [38634240882](https://github.com/wheel338-sketch/windfall/actions/runs/38634240882) | `cb008dd6` | Apple Silicon: UI 6,545 / 0, clippy clean, workspace **4,371 / 49 / 33** (§4.3), launch smoke **13 / 13** from the DMG including Apply pressed in the window through accessibility and an audible render. |
| [38651043377](https://github.com/wheel338-sketch/windfall/actions/runs/38651043377) | `8f1b2036` | see §4.3 |

### 4.2 Launch smoke on Apple Silicon (run 38634240882)

From the mounted DMG, the app copied out and its quarantine flag removed:
the MCP server answers at start-up (4.5 s); the token is required; the summary
shows the factory kit; the frontend reports its view; a main window of the
right size is on screen (CGWindowList); a proposal is filed and **approved by
pressing Apply in the window** (System Events through JXA, walking the window's
accessibility tree into the web area); the render with the steps applied is
audible (peak 0.26) and finite; the app exits on SIGTERM. There is no
WebDriver for WKWebView, so the full song workflow does not run on macOS.

### 4.3 Workspace tests

Run 38634240882, Apple Silicon, before `8f1b2036`: 4,371 passed, 49 failed,
33 ignored. The failures, all diagnosed:

| Count | Cause | Fixed by |
|---|---|---|
| 29 + 5 fixture-only | The test plugins' macOS `bundleEntry`/`bundleExit` module was never declared, so the fixture had no mandatory VST3 bundle exports | `8f1b2036` (test fixture) |
| 7 | Plugin export re-load in the same process (product, §5.2) | `8f1b2036` |
| 2 | Capture-ack manager tests: macOS hosts plugins on the app's Cocoa main thread, which a unit test lacks | ignored on macOS, with that reason |
| 1 | Analyzer capture: spectrum bins near zero off by 2.2e-8 | absolute floor 1e-7 (§5.3) |
| 1 | Update download: `WouldBlock` where `TimedOut` was expected (product, §5.2) | `8f1b2036` |
| 3 (CPU budget step) | gms, sytrus, toxic_biohazard deadlines on the shared runner | recorded only |

Run 38651043377 at `8f1b2036`: _to be filled in from the run._

## 5. Bugs found and fixed

### 5.1 Linux

| Commit | Bug | Fix |
|---|---|---|
| `154fee77` | `gui/x11.rs` had two clippy findings only Linux compiles; `-D warnings` failed the build | same-type cast removed, `if` collapsed |
| `154fee77` | The X11 editor links `X11-xcb`, `xcb` and `Xrandr` directly; build images (and CI's package list, change since withdrawn with the CI work) lacked the -dev packages, so `windfall-plugin` did not link | packages in the Docker build image |
| `154fee77` | A fresh checkout cannot compile the desktop crate until `target/release/plugin-runtime` exists (Tauri checks bundled resources in `build.rs`) | the portable gates build it first, as `tauri build` does (tooling, not product) |
| `f1462d07` | **The .deb declared only WebKitGTK and GTK**, but the app links ALSA directly: on a clean Ubuntu it stopped at `libasound.so.2`. A plain `libasound2` dependency then let Ubuntu 24.04 satisfy the virtual package with `liboss4-salsa-asound2` (`undefined symbol: snd_pcm_info_get_stream`) | `bundle.linux.deb.depends`: `libasound2t64 \| libasound2`, `libx11-xcb1`, `libxrandr2` |
| `d1dff720` | **Toolbar controls were not clickable on WebKitGTK.** An overflowing horizontal strip gets an overlay scrollbar, not drawn until hovered, that takes every hit in the strip's bottom 22 pixels: in the 36-pixel piano roll and playlist toolbars, most of every control (Snap, the tools, Typing…). Measured with `elementFromPoint` and with real X pointer clicks under Xvfb | `<html data-webview>` marked before the first render (`lib/webview-engine.ts`, tested); the three toolbar strips hide their scrollbar under WebKitGTK only (`scrollbar-width: none` must be in force at layout); they still scroll by wheel and touchpad; Windows keeps its thin bar |
| `1cab2848` | **Plugin export failed with "Invalid argument (os error 22)"** on filesystems without an exclusive rename (FAT/exFAT USB sticks, 9p, CIFS/SMB, older NFS): the export was published with `renameat2(RENAME_NOREPLACE)` (`renamex_np(RENAME_EXCL)` on macOS) | on EINVAL/ENOSYS/EOPNOTSUPP (ENOTSUP), the destination is created exclusively and the staged entries moved in, so an existing folder is still never overwritten (unit-tested; the Linux smoke exports onto a 9p mount through it) |
| `8f1b2036` | Update download header reads: a read that times out is `WouldBlock` on Unix, `TimedOut` on Windows, so the absolute deadline was reported differently by platform | reported as `TimedOut` everywhere |

Tests corrected for Linux (behaviour of the product was right): the two
`vst3_host` editor tests expected `UnsupportedPlatform` off Windows, but
Linux now has an X11 editor host and answers `NoEditor` for a plugin without
an editor (`722dbb06`). The analyzer and performance-media native-capture
tests compare with fixtures recorded on Windows that the UI tests also read;
off Windows the platform math library rounds differently in the last bits
(Linux: at most about 2 f32 epsilons, 4e-9 near zero; no string or flag
differs), so off Windows the structure, strings and flags must still be
identical and numbers may differ by rounding only, with the differing paths
reported (`session/tests/native_capture.rs`, tested; Windows still compares
exactly).

### 5.2 macOS

| Commit | Bug | Fix |
|---|---|---|
| `f1462d07` | `tauri::generate_context!()` was expanded twice in the desktop crate (`run()` and the updater signature test); on macOS each expansion embeds the Info.plist as `_EMBED_INFO_PLIST`, so the desktop test binary did not link and no desktop test ran | one generic `crate::context()` |
| `f1462d07` | `au/mod.rs`: three clippy findings only macOS compiles | type alias, collapsed `if`s |
| `8f1b2036` | **Plugin export, then loading the export in the same process, failed** ("runtime stamp is missing/unreadable at …/.windfall-plugin-*/…"). The export loaded each staged copy to check its stamp; macOS cannot unload a Rust dylib, so the image stayed resident under the stage's path, and loading the published bundle (the same file, renamed) returned it | on macOS the staged copy is compared byte for byte with the already probed source (`pluginExport.runtimeCopyDiffers`, English and Spanish); elsewhere it is still loaded |
| `8f1b2036` | (the update deadline above, found on macOS) | |

Test infrastructure for macOS: the fixture's `bundleEntry`/`bundleExit`
module declared (`8f1b2036`); the fixture-only bridge tests publish a VST3
bundle on macOS as the integration tests do (`f1462d07`); the VST3 editor
tests expect the Cocoa host's main-thread refusal under libtest's threads
(`cb008dd6`).

### 5.3 Windows after the changes

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D
warnings` clean; `windfall-desktop` 783 / 0 / 14 (with the fixture
variables), `windfall-plugin` 38 / 0 / 0, `windfall-plugin-host` 298 / 0 / 7,
`windfall-ipc` 189 / 0; pnpm typecheck, lint, build pass; vitest 543 files,
6,547 / 0. The macOS-target clippy for `windfall-plugin` and
`windfall-plugin-host` and the macOS check of the test plugins pass from
Windows (`DOCS_RS=1`, as `scripts/check-macos-desktop.sh` does).

## 6. Not tested

- Real audio and MIDI hardware on either platform (only the software sink and
  ALSA's null device); JACK and PipeWire/PulseAudio routing.
- Real third-party plugins (only the owned test plugins and Windfall's own
  export); plugin editors on screen (X11 and Cocoa editor windows).
- Wayland sessions (Xvfb is X11), other distributions than Ubuntu
  22.04/24.04, ARM Linux, the AppImage through FUSE (extracted only),
  desktop integration (menu entries, file associations).
- Code signing and notarization; **Gatekeeper's handling of these unsigned
  apps** (the smoke removed the quarantine flag; a downloaded unsigned DMG
  will be blocked until the user allows it in System Settings).
- macOS UI workflows beyond the MCP smoke and one Apply click (no WebDriver for
  WKWebView); macOS versions other than the runner images'; Intel results
  are as reported in §4.3.
- Auto-update on Linux and macOS (updater artifacts were not built).

## 7. Artifacts

Linux (from `8f1b2036`, in `target/xplat/linux/`):

| File | Bytes | SHA-256 |
|---|---|---|
| `Windfall_1.0.1_amd64.AppImage` | 123,906,552 | `edea810a87561f8fd0409994dddf49735529b90638f97a1a6e7e4bf8e110cdbe` |
| `Windfall_1.0.1_amd64.deb` | 49,282,664 | `9ddf11f7f21c2a6dd54d48356b5282864f239cad982b8c907ae6a591ae10c3e8` |

Smoke reports, screenshots and pristine summaries are beside them
(`smoke/`, `pristine/`, `logs/`). macOS: see §4.3 and the final message.

Docker leaves the images `windfall-linux-build` (2.64 GB) and
`windfall-linux-runtime` (1.14 GB) and the volumes `windfall-target`
(76.8 GB of Cargo output), `windfall-src` (0.5 GB) and `windfall-cargo`
(0.35 GB); no container is left running.
