#!/usr/bin/env bash
# Runs inside a plain Ubuntu image (no build tools, no test tools), started
# by `scripts/linux/xplat.sh pristine`: is everything Windfall's packages
# load present once they are installed the way a user installs them?
#
#   AppImage first, on the clean system with only its documented host
#   requirement installed: ALSA's libasound2, which AppImages leave to the
#   host on purpose (linuxdeploy's exclude list) so the host's ALSA setup
#   applies; every desktop distribution has it. Extracted (no FUSE), every
#   ELF file through ldd with the AppImage's own libraries, then the app
#   started without a display (it must get as far as GTK refusing to open
#   one, not stop at a missing library). gdk-pixbuf's image loaders are
#   optional plugins GTK skips when their library is absent (SVG needs the
#   host's librsvg, TIFF its libtiff); they are listed, not failed.
#   .deb second: apt install, then the same checks on the installed files.
set -uo pipefail
artifacts="${1:?artifacts folder}"
release=$(. /etc/os-release && echo "$VERSION_ID")
out="$artifacts/pristine/ubuntu-$release"
rm -rf "$out"; mkdir -p "$out"
status=0
note() { echo "$*" | tee -a "$out/summary.log"; }
export DEBIAN_FRONTEND=noninteractive
ls "$artifacts"/*.deb "$artifacts"/*.AppImage > /dev/null || { note "no .deb or AppImage in $artifacts"; exit 2; }
apt-get update > "$out/apt-update.log" 2>&1
apt-get install -y --no-install-recommends file > /dev/null 2>&1

missing() { # ELF files read from stdin whose libraries ldd cannot find; $1 the library path
  local found=0
  while IFS= read -r file; do
    if file -b "$file" | grep -q '^ELF'; then
      local lost
      lost=$(LD_LIBRARY_PATH="${1:-}" ldd "$file" 2>&1 | grep 'not found')
      if [ -n "$lost" ]; then note "   $file:"; echo "$lost" | sed 's/^/     /' | tee -a "$out/summary.log"; found=1; fi
    fi
  done
  return $found
}
started_to_gtk() { # the app run with no display: which error stops it
  local log="$out/$1-nodisplay.log"
  shift
  env -u DISPLAY -u WAYLAND_DISPLAY timeout 20 "$@" > "$log" 2>&1
  local s=$?
  if grep -qE 'error while loading shared libraries|symbol lookup error' "$log"; then note "   stops at a missing library or symbol:"; head -3 "$log" | tee -a "$out/summary.log"; return 1; fi
  if ! grep -q 'Failed to initialize GTK' "$log"; then note "   did not reach GTK (exit $s):"; head -5 "$log" | tee -a "$out/summary.log"; return 1; fi
  note "   without a display it stops at GTK's display check (exit $s), as it should"
}

image=$(ls "$artifacts"/*.AppImage | head -1)
note "== Ubuntu $release: $(basename "$image"), extracted, with the host's ALSA only"
alsa=libasound2; [ "$release" = 24.04 ] && alsa=libasound2t64
apt-get install -y --no-install-recommends "$alsa" > "$out/apt-alsa.log" 2>&1 || { note "   $alsa could not be installed"; status=1; }
work=$(mktemp -d)
cp "$image" "$work/Windfall.AppImage"; chmod +x "$work/Windfall.AppImage"
(cd "$work" && ./Windfall.AppImage --appimage-extract > /dev/null)
root="$work/squashfs-root"
libraries="$root/usr/lib:$root/usr/lib/x86_64-linux-gnu"
find "$root" -type f -not -path '*/gdk-pixbuf-2.0/*/loaders/*' | missing "$libraries" || { note "   unresolved libraries above"; status=1; }
optional=$(find "$root" -type f -path '*/gdk-pixbuf-2.0/*/loaders/*' | while IFS= read -r file; do
  LD_LIBRARY_PATH="$libraries" ldd "$file" 2>&1 | grep -q 'not found' && echo "$(basename "$file") ($(LD_LIBRARY_PATH="$libraries" ldd "$file" | awk '/not found/ {print $1}' | tr '\n' ' '))"
done)
[ -n "$optional" ] && note "   optional image loaders without their host library: $(echo $optional)"
started_to_gtk appimage "$root/AppRun" || status=1

deb=$(ls "$artifacts"/*.deb | head -1)
note "== Ubuntu $release: apt install $(basename "$deb")"
if apt-get install -y --no-install-recommends "$deb" > "$out/apt-install.log" 2>&1; then
  note "   installed: $(dpkg-query -W -f '${Package} ${Version} ${Installed-Size}KiB' windfall)"
  note "   depends: $(dpkg-query -W -f '${Depends}' windfall)"
  note "   ALSA provided by: $(dpkg-query -W -f '${Package} ${Version}\n' 'libasound2*' 2>/dev/null | grep -v '^$' | tr '\n' ' ')$(dpkg -l 'liboss4-salsa*' 2>/dev/null | grep -q '^ii' && echo ' and liboss4-salsa (wrong)')"
  dpkg -L windfall | missing || { note "   unresolved libraries above"; status=1; }
  started_to_gtk deb /usr/bin/windfall-desktop || status=1
  /usr/bin/windfall-desktop --mcp-stdio < /dev/null > "$out/deb-mcp-stdio.log" 2>&1
  code=$?
  note "   --mcp-stdio with no app running: exit $code ($(head -c 200 "$out/deb-mcp-stdio.log"))"
  [ "$code" -eq 1 ] && grep -q 'MCP server is off' "$out/deb-mcp-stdio.log" || status=1
else
  note "   apt install failed"; tail -20 "$out/apt-install.log" | tee -a "$out/summary.log"; status=1
fi
note "== exit $status"
exit $status
