#!/usr/bin/env bash
# Runs inside a plain Ubuntu image (no build tools, no test tools), started
# by `scripts/linux/xplat.sh pristine`: is everything Windfall's packages
# load present once they are installed the way a user installs them?
#
#   .deb:     apt install, then every ELF file of the package through ldd,
#             then the app started without a display (it must get as far as
#             GTK refusing to open one, not stop at a missing library).
#   AppImage: extracted (no FUSE), the same checks on its own libraries.
set -uo pipefail
artifacts="${1:?artifacts folder}"
release=$(. /etc/os-release && echo "$VERSION_ID")
out="$artifacts/pristine/ubuntu-$release"
rm -rf "$out"; mkdir -p "$out"
status=0
note() { echo "$*" | tee -a "$out/summary.log"; }
export DEBIAN_FRONTEND=noninteractive
apt-get update > "$out/apt-update.log" 2>&1
apt-get install -y --no-install-recommends file > /dev/null 2>&1

missing() { # ELF files under $1 with libraries ldd cannot find
  local found=0
  while IFS= read -r file; do
    if file -b "$file" | grep -q '^ELF'; then
      local lost
      lost=$(LD_LIBRARY_PATH="${2:-}" ldd "$file" 2>&1 | grep 'not found')
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
  if grep -q 'error while loading shared libraries' "$log"; then note "   stops at a missing library:"; head -3 "$log" | tee -a "$out/summary.log"; return 1; fi
  note "   without a display it stops with exit $s: $(grep -m1 -iE 'display|gtk' "$log" | cut -c1-160)"
}

deb=$(ls "$artifacts"/*.deb | head -1)
note "== Ubuntu $release: apt install $(basename "$deb")"
if apt-get install -y --no-install-recommends "$deb" > "$out/apt-install.log" 2>&1; then
  note "   installed: $(dpkg-query -W -f '${Package} ${Version} ${Installed-Size}KiB' windfall)"
  note "   depends: $(dpkg-query -W -f '${Depends}' windfall)"
  dpkg -L windfall | missing || { note "   unresolved libraries above"; status=1; }
  started_to_gtk deb /usr/bin/windfall-desktop || status=1
  /usr/bin/windfall-desktop --mcp-stdio < /dev/null > "$out/deb-mcp-stdio.log" 2>&1
  note "   --mcp-stdio with no app running: exit $? ($(head -c 200 "$out/deb-mcp-stdio.log"))"
else
  note "   apt install failed"; tail -20 "$out/apt-install.log" | tee -a "$out/summary.log"; status=1
fi
apt-get remove -y windfall > /dev/null 2>&1

image=$(ls "$artifacts"/*.AppImage | head -1)
note "== Ubuntu $release: $(basename "$image"), extracted"
work=$(mktemp -d)
cp "$image" "$work/Windfall.AppImage"; chmod +x "$work/Windfall.AppImage"
(cd "$work" && ./Windfall.AppImage --appimage-extract > /dev/null)
root="$work/squashfs-root"
find "$root" -type f | missing "$root/usr/lib:$root/usr/lib/x86_64-linux-gnu" || { note "   unresolved libraries above (with the AppImage's own usr/lib)"; status=1; }
started_to_gtk appimage "$root/AppRun" || status=1
note "== exit $status"
exit $status
