#!/usr/bin/env bash
# Builds and tests Windfall for Linux in Docker, from a Windows (Git Bash),
# Linux or macOS host. See scripts/linux/README.md.
#
#   scripts/linux/xplat.sh images          build both images
#   scripts/linux/xplat.sh sync            copy the checkout into the source volume
#   scripts/linux/xplat.sh gates [STEP...] scripts/xplat/gates.sh in the build image
#   scripts/linux/xplat.sh bundle          unsigned .deb and AppImage
#   scripts/linux/xplat.sh export          bundles and logs to target/xplat/linux
#   scripts/linux/xplat.sh smoke           the installed app, in the runtime image
#   scripts/linux/xplat.sh shell           a build shell
#
# Volumes: windfall-src (the copied source), windfall-target (Cargo's target
# folder, so rebuilds are incremental and never mix with the host's
# target/), windfall-cargo (registry) and windfall-pnpm (store).
# Containers are capped at XPLAT_CPUS (12) CPUs and XPLAT_MEMORY (13g).
set -euo pipefail
export MSYS_NO_PATHCONV=1
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
if command -v cygpath >/dev/null; then host_repo="$(cygpath -w "$repo")"; else host_repo="$repo"; fi
cpus="${XPLAT_CPUS:-12}"
memory="${XPLAT_MEMORY:-13g}"
jobs="${XPLAT_JOBS:-8}"
build_image=windfall-linux-build
runtime_image=windfall-linux-runtime
volumes=(-v windfall-src:/src -v windfall-target:/src/target -v windfall-cargo:/opt/cargo/registry -v windfall-pnpm:/root/.local/share/pnpm)
run_build() {
  docker run --rm --init --cpus "$cpus" --memory "$memory" "${volumes[@]}" \
    -e CARGO_BUILD_JOBS="$jobs" -e CI=true -w /src "$build_image" "$@"
}

command="${1:-}"
shift || true
case "$command" in
  images)
    docker build -f "$here/Dockerfile" --target build -t "$build_image" "$here"
    docker build -f "$here/Dockerfile" --target runtime -t "$runtime_image" "$here"
    ;;
  sync)
    # Working-tree files, not a commit, so uncommitted fixes are tested too.
    docker run --rm --cpus "$cpus" -v "$host_repo:/host:ro" "${volumes[@]}" "$build_image" \
      rsync -a --delete --exclude=/target --exclude=node_modules --exclude=/.git \
        --exclude=/.claude --exclude=/apps/desktop/dist --exclude=/apps/desktop/src-tauri/gen/schemas \
        --exclude=/crates/windfall-plugin-host/test-plugins/target /host/ /src/
    ;;
  gates)
    run_build bash scripts/xplat/gates.sh target/xplat-logs "$@"
    ;;
  bundle)
    # Unsigned preview: --no-sign skips the updater signature, so no key is
    # read; createUpdaterArtifacts is switched off to match.
    run_build bash -c 'set -euo pipefail
      cd apps/desktop
      pnpm install --frozen-lockfile
      pnpm tauri build --ci --no-sign --bundles deb,appimage \
        --config "{\"bundle\":{\"createUpdaterArtifacts\":false}}"
      ls -la /src/target/release/bundle/deb /src/target/release/bundle/appimage'
    ;;
  export)
    out="$repo/target/xplat/linux"
    rm -rf "$out"; mkdir -p "$out"
    id=$(docker create "${volumes[@]}" "$build_image" true)
    docker cp "$id:/src/target/release/bundle/deb/." "$out/" || true
    docker cp "$id:/src/target/release/bundle/appimage/." "$out/" || true
    docker cp "$id:/src/target/xplat-logs" "$out/logs" || true
    docker rm "$id" >/dev/null
    rm -rf "$out"/*.AppDir "$out"/Windfall.AppDir "$out"/build_appimage.sh 2>/dev/null || true
    ls -la "$out"
    ;;
  smoke)
    # A fresh runtime container: no toolchain, nothing from the build volume
    # except the packages and the smoke scripts.
    out="$repo/target/xplat/linux"
    host_out="$out"; command -v cygpath >/dev/null && host_out="$(cygpath -w "$out")"
    host_scripts="$repo/scripts"; command -v cygpath >/dev/null && host_scripts="$(cygpath -w "$host_scripts")"
    docker run --rm --init --cpus "$cpus" --memory "$memory" --shm-size 1g \
      -v "$host_out:/artifacts" -v "$host_scripts:/scripts:ro" \
      "$runtime_image" bash /scripts/linux/smoke.sh /artifacts "$@"
    ;;
  shell)
    docker run --rm -it --init --cpus "$cpus" --memory "$memory" "${volumes[@]}" -w /src "$build_image" bash
    ;;
  *)
    sed -n '2,17p' "$0"; exit 2 ;;
esac
