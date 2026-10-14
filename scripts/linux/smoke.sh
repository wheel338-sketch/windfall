#!/usr/bin/env bash
# Runs inside the runtime image (scripts/linux/Dockerfile, target runtime),
# started by `scripts/linux/xplat.sh smoke`. ARTIFACTS holds the .deb, the
# AppImage and fixtures/libwindfall_test_plugins.so; reports go to
# ARTIFACTS/smoke. The app runs on its software audio sink, and ALSA's
# default device is the null device, so nothing reaches real hardware.
#
# 1. The AppImage, extracted (no FUSE in a container), before any Windfall
#    package is installed: the MCP launch smoke and the song workflow. The
#    host provides ALSA's libasound.so.2, as every desktop distribution does:
#    AppImages leave it out on purpose (linuxdeploy's exclude list) so the
#    host's ALSA configuration applies, so it is installed first here.
# Each smoke has a hard timeout (4 and 15 minutes); a timeout is exit 124.
# 2. The .deb, installed with apt so its declared dependencies come in: the
#    same two smokes against /usr/bin/windfall-desktop.
#
# Optional second argument: the runs to do, of appimage-mcp appimage-song
# deb-mcp deb-song (all by default).
set -uo pipefail
artifacts="${1:?artifacts folder}"
only="${2:-appimage-mcp appimage-song deb-mcp deb-song}"
wanted() { [[ " $only " == *" $1 "* ]]; }
scripts="$(cd "$(dirname "$0")/.." && pwd)"
out="$artifacts/smoke"
rm -rf "$out"; mkdir -p "$out"
status=0
summary="$out/summary.log"
note() { echo "$*" | tee -a "$summary"; }

deb=$(ls "$artifacts"/*.deb | head -1)
image=$(ls "$artifacts"/*.AppImage | head -1)
fixture="$artifacts/fixtures/libwindfall_test_plugins.so"

# A display, a window manager (the smoke closes the window as a user does,
# through it), and a session bus as a desktop has.
export DISPLAY=:99
Xvfb :99 -screen 0 1600x1000x24 -nolisten tcp > "$out/xvfb.log" 2>&1 &
for _ in $(seq 50); do xdpyinfo > /dev/null 2>&1 && break; sleep 0.2; done
xdpyinfo | grep dimensions | tee -a "$summary"
chmod 1777 /tmp
# Runs "$@" as the unprivileged user, in a session bus with openbox running.
run_as_tester() {
  local script
  script=$(mktemp /tmp/smoke-XXXXXX.sh)
  {
    echo "export DISPLAY=:99 PATH='$PATH' SMOKE_PLUGIN_FIXTURE='$fixture'"
    echo 'cd /home/tester'
    echo 'openbox > /tmp/openbox-$$.log 2>&1 &'
    echo 'wm=$!; sleep 1'
    printf '%q ' "$@"; echo
    echo 's=$?; kill $wm; exit $s'
  } > "$script"
  chmod 755 "$script"
  su tester -c "dbus-run-session -- bash $script"
}
chmod -R a+rwX "$out"

smoke() {
  local label="$1" exe="$2" s
  if wanted "$label-mcp"; then
  note "== $label: MCP launch smoke ($exe)"
  run_as_tester timeout --kill-after=15 240 node "$scripts/xplat/mcp-launch-smoke.mjs" "$exe" "$out/$label-mcp/report.json" > "$out/$label-mcp.log" 2>&1
  s=$?
  tail -25 "$out/$label-mcp.log" | tee -a "$summary"
  note "   exit $s"; [ $s -eq 0 ] || status=1
  fi
  wanted "$label-song" || return 0
  note "== $label: song workflow smoke (WebDriver)"
  run_as_tester timeout --kill-after=15 900 node "$scripts/song-workflow-window-smoke.mjs" "$exe" "$out/$label-song/report.json" 4444 > "$out/$label-song.log" 2>&1
  s=$?
  tail -5 "$out/$label-song.log" | tee -a "$summary"
  node -e "const r=require('$out/$label-song/report.json'); for (const c of r.checks) console.log((c.pass?'PASS ':'FAIL ')+c.name); if (r.error) console.log(r.error.slice(0,3000))" 2>&1 | tee -a "$summary"
  note "   exit $s"; [ $s -eq 0 ] || status=1
}

# --- 1. The AppImage. ---
apt-get update > "$out/apt-update.log" 2>&1
apt-get install -y --no-install-recommends libasound2 > "$out/apt-alsa.log" 2>&1 || { note "libasound2 could not be installed"; status=1; }
appimage_root=/opt/windfall-appimage
rm -rf "$appimage_root"; mkdir -p "$appimage_root"
cp "$image" "$appimage_root/Windfall.AppImage"; chmod +x "$appimage_root/Windfall.AppImage"
(cd "$appimage_root" && ./Windfall.AppImage --appimage-extract > /dev/null) || { note "AppImage extraction failed"; status=1; }
chmod -R a+rX "$appimage_root"
smoke appimage "$appimage_root/squashfs-root/AppRun"

# --- 2. The .deb. ---
note "== deb: apt install $(basename "$deb")"
apt-get install -y --no-install-recommends "$deb" > "$out/apt-install.log" 2>&1 || { note "apt install failed"; tail -20 "$out/apt-install.log" | tee -a "$summary"; status=1; }
dpkg -L windfall | grep -E 'bin/|plugin-runtime|factory/[^/]+$' | head -20 | tee -a "$summary"
smoke deb /usr/bin/windfall-desktop

note "== overall: exit $status"
chmod -R a+rwX "$out"
exit $status
