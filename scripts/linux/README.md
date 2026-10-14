# Linux builds and tests in Docker

Windfall's Linux packages are built and tested in containers on Ubuntu 22.04,
the oldest distribution supported: a .deb and an AppImage built there run on
newer releases too. Everything here runs from Git Bash on Windows as well as
from a Linux or macOS shell; Docker must be running.

```sh
scripts/linux/xplat.sh images     # windfall-linux-build and windfall-linux-runtime
scripts/linux/xplat.sh sync       # the working tree (not a commit) into the source volume
scripts/linux/xplat.sh gates      # scripts/xplat/gates.sh: fmt, UI checks, clippy, fixtures, tests
scripts/linux/xplat.sh bundle     # unsigned .deb and AppImage
scripts/linux/xplat.sh export     # bundles and logs to target/xplat/linux
scripts/linux/xplat.sh smoke      # the AppImage, then the installed .deb, in the runtime image
scripts/linux/xplat.sh pristine   # the packages in plain Ubuntu 22.04 and 24.04
```

`export` also needs the owned test plugins for the smoke's plugin phase:
copy `target/debug/libwindfall_test_plugins.so` from the target volume to
`target/xplat/linux/fixtures/` (the `export` step does).

## Images and volumes

- `windfall-linux-build`: Rust (the version the release workflow pins),
  Node 24, pnpm 11, the -dev packages Tauri 2 and the plugin editors link
  against, Xvfb and `tauri-driver`.
- `windfall-linux-runtime`: a fresh Ubuntu 22.04 with no compiler and no
  -dev packages. The smoke installs the .deb with apt, so the package's
  declared dependencies are what bring the libraries in. Xvfb, openbox,
  `WebKitWebDriver`, `tauri-driver`, xdotool, wmctrl and Node are test tools.
- Volumes `windfall-src`, `windfall-target`, `windfall-cargo` and
  `windfall-pnpm` keep the copied source, Cargo's target folder, the
  registry and the pnpm store, so rebuilds are incremental and never mix
  with the host's `target/`. `docker volume rm` them to start over.

Containers are capped with `--cpus`/`--memory` (`XPLAT_CPUS`, default 12;
`XPLAT_MEMORY`, 13g) and Cargo uses `XPLAT_JOBS` (8) jobs.

## Unsigned preview bundles

`bundle` runs `tauri build --no-sign` with `createUpdaterArtifacts` off: no
updater key is read and none may be given to these images. See
`docs/RELEASE-ENGINEERING.md`.

## The smokes

The app runs on its software audio sink (`WINDFALL_SOFTWARE_AUDIO=1`) with a
profile folder of its own, and ALSA's default device in the runtime image is
the null device, so nothing reaches audio hardware.

- `scripts/xplat/mcp-launch-smoke.mjs`: starts the app with its MCP server
  left on in the profile and checks start-up, the factory kit, the frontend,
  the window, a render, and the token, over loopback.
- `scripts/song-workflow-window-smoke.mjs`: the Windows song workflow,
  driven on Linux through WebDriver (WebKitGTK has no DevTools protocol),
  with GTK's own file chooser answered by xdotool and the window closed
  through the window manager. With `SMOKE_PLUGIN_FIXTURE` it also exports the
  song as a plugin from the installed runtime and hosts the owned CLAP
  fixture.
