#!/usr/bin/env bash
# Builds and tests Windfall for Linux in Docker, from a Windows (Git Bash),
# Linux or macOS host. See scripts/linux/README.md.
#
#   scripts/linux/xplat.sh images          build both images
#   scripts/linux/xplat.sh sync            copy the checkout into the source volume
#   scripts/linux/xplat.sh gates [STEP...] scripts/xplat/gates.sh in the build image
#   scripts/linux/xplat.sh bundle          unsigned .deb and AppImage
#   scripts/linux/xplat.sh export          bundles and logs to target/xplat/linux
#   scripts/linux/xplat.sh smoke           the AppImage and the installed .deb, in the runtime image
#   scripts/linux/xplat.sh pristine        the packages in plain Ubuntu 22.04 and 24.04
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
# Docker Desktop on Windows takes Windows paths for bind mounts and contexts.
host() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
host_repo="$(host "$repo")"
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
    docker build -f "$(host "$here/Dockerfile")" --target build -t "$build_image" "$(host "$here")"
    docker build -f "$(host "$here/Dockerfile")" --target runtime -t "$runtime_image" "$(host "$here")"
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
    docker cp "$id:/src/target/release/bundle/deb/." "$(host "$out")/" || true
    docker cp "$id:/src/target/release/bundle/appimage/." "$(host "$out")/" || true
    docker cp "$id:/src/target/xplat-logs" "$(host "$out/logs")" || true
    mkdir -p "$out/fixtures"
    docker cp "$id:/src/target/debug/libwindfall_test_plugins.so" "$(host "$out/fixtures")/" || true
    docker rm "$id" >/dev/null
    # Only the packages: not the staging folders Tauri leaves beside them.
    find "$out" -mindepth 1 -maxdepth 1 -type d ! -name fixtures ! -name logs -exec rm -rf {} +
    ls -la "$out"
    ;;
  smoke)
    # A fresh runtime container: no toolchain, nothing from the build volume
    # except the packages and the smoke scripts.
    out="$repo/target/xplat/linux"
    host_out="$(host "$out")"; host_scripts="$(host "$repo/scripts")"
    # The smokes time out on their own; this bounds the whole container
    # (45 minutes) and removes it whatever happens.
    docker rm -f windfall-linux-smoke > /dev/null 2>&1 || true
    status=0
    timeout 2700 docker run --rm --init --name windfall-linux-smoke --cpus "$cpus" --memory "$memory" --shm-size 1g \
      -v "$host_out:/artifacts" -v "$host_scripts:/scripts:ro" \
      "$runtime_image" bash /scripts/linux/smoke.sh /artifacts "$@" || status=$?
    docker rm -f windfall-linux-smoke > /dev/null 2>&1 || true
    [ "$status" -eq 124 ] && echo "The Linux smoke container ran past 45 minutes and was removed" >&2
    exit "$status"
    ;;
  pristine)
    # Plain Ubuntu images, oldest and newest LTS: only what apt brings in.
    out="$repo/target/xplat/linux"
    host_out="$(host "$out")"; host_scripts="$(host "$repo/scripts")"
    status=0
    for release in 22.04 24.04; do
      docker run --rm --cpus "$cpus" --memory "$memory" -v "$host_out:/artifacts" -v "$host_scripts:/scripts:ro" \
        "ubuntu:$release" bash /scripts/linux/pristine.sh /artifacts || status=1
    done
    exit $status
    ;;
  shell)
    docker run --rm -it --init --cpus "$cpus" --memory "$memory" "${volumes[@]}" -w /src "$build_image" bash
    ;;
  *)
    sed -n '2,18p' "$0"; exit 2 ;;
esac
