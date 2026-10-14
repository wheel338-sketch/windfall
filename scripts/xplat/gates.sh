#!/usr/bin/env bash
# The repository's Rust and UI gates for Linux and macOS, in the order the
# Windows gates run them, with the owned plugin fixtures built first so the
# fixture-backed tests run instead of skipping. Used by the Linux container
# (scripts/linux/xplat.sh); written for macOS too, where nothing has been
# supported yet.
#
#   scripts/xplat/gates.sh LOG_FOLDER [STEP...]
#
# Steps: fmt ui-install typecheck lint vitest frontend-build plugin-runtime clippy
#        fixtures native-bins test-workspace test-fixtures-ignored
# The UI is built first: the desktop crate embeds apps/desktop/dist.
# With no steps, all run. Each step writes LOG_FOLDER/STEP.log ending in an
# `EXIT:` line; LOG_FOLDER/summary.log lists every step and the test totals.
# A failed step does not stop the next one; the script exits non-zero if any
# step failed.
set -uo pipefail
cd "$(dirname "$0")/../.."
logs="${1:?log folder}"
shift
mkdir -p "$logs"
logs="$(cd "$logs" && pwd)"
root="$PWD"

case "$(uname -s)" in
  Darwin) dll=dylib ;;
  Linux) dll=so ;;
  *) echo "Linux or macOS only" >&2; exit 2 ;;
esac
export TS_RS_EXPORT_DIR="${TS_RS_EXPORT_DIR:-$(mktemp -d)}"
export WINDFALL_BRIDGE_FIXTURE="$root/target/debug/libwindfall_test_plugins.$dll"
export WINDFALL_DESKTOP_BRIDGE_HELPER="$root/target/debug/windfall-desktop"
export WINDFALL_BRIDGE_HELPER="$root/target/debug/windfall-plugin-audio"
export WINDFALL_EXPORT_FIXTURE="$root/target/debug/libwindfall_plugin.$dll"
export CARGO_TERM_COLOR=never

failed=0
summary="$logs/summary.log"
step() {
  local name="$1"; shift
  local log="$logs/$name.log" started status
  started=$(date +%s)
  echo "== $name: $*" | tee -a "$summary"
  { echo "COMMAND: $*"; "$@"; } > "$log" 2>&1
  status=$?
  echo "EXIT: $status" >> "$log"
  echo "ELAPSED: $(( $(date +%s) - started ))s" >> "$log"
  local totals
  totals=$(awk '/^test result:/ { for (i = 1; i <= NF; i++) { if ($(i+1) ~ /^passed/) p += $i; if ($(i+1) ~ /^failed/) f += $i; if ($(i+1) ~ /^ignored/) g += $i }; b++ } END { if (b) printf " tests %d/%d/%d over %d blocks", p, f, g, b }' "$log")
  echo "   $name: exit $status, $(( $(date +%s) - started ))s$totals" | tee -a "$summary"
  if [ "$status" -ne 0 ]; then
    [ -n "${REPORT_ONLY:-}" ] && echo "   $name: recorded, not gating ($REPORT_ONLY)" | tee -a "$summary" || failed=1
    grep -E '^(test .* FAILED|---- .* stdout ----|error(\[|:))' "$log" | head -40 | sed 's/^/     /' | tee -a "$summary"
  fi
  return 0
}

all=(fmt ui-install typecheck lint vitest frontend-build plugin-runtime clippy fixtures native-bins test-workspace test-fixtures-ignored)
selected=("$@")
[ ${#selected[@]} -eq 0 ] && selected=("${all[@]}")

for name in "${selected[@]}"; do
  case "$name" in
    fmt) step fmt cargo fmt --all --check ;;
    clippy) step clippy cargo clippy --workspace --all-targets -- -D warnings ;;
    fixtures) step fixtures cargo build --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target ;;
    native-bins) step native-bins cargo build -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib ;;
    # The instruments' CPU-deadline tests hold a synth to a share of one
    # performance core. On shared machines (XPLAT_CPU_BUDGET=report)
    # they run as their own step and are recorded without deciding the
    # gate; everywhere else they gate with the rest.
    test-workspace)
      if [ "${XPLAT_CPU_BUDGET:-gate}" = report ]; then
        step test-workspace cargo test --workspace --no-fail-fast -- --test-threads=1 --skip instrument_deadlines::
        REPORT_ONLY="shared runner, not a performance core" step cpu-budget cargo test -p windfall-dsp --test dsp --no-fail-fast -- --test-threads=1 instrument_deadlines::
      else
        step test-workspace cargo test --workspace --no-fail-fast -- --test-threads=1
      fi
      ;;
    # The cases ignored only because they need the fixture library built by
    # hand; the workspace run above leaves them out. (The desktop crate's
    # native plugin tests, plugins::capture_ack_tests and friends, are
    # compiled on Windows only.)
    test-fixtures-ignored)
      step test-fixtures-ignored-host cargo test -p windfall-plugin-host --lib --no-fail-fast -- --ignored --test-threads=1 bridge::helper clap::instance::state_tests
      ;;
    ui-install) step ui-install pnpm --dir apps/desktop install --frozen-lockfile ;;
    typecheck) step typecheck pnpm --dir apps/desktop typecheck ;;
    lint) step lint pnpm --dir apps/desktop lint ;;
    # On machines with few cores, VITEST_WORKERS=2 keeps the UI tests inside
    # their own timeouts.
    vitest) step vitest pnpm --dir apps/desktop exec vitest run --maxWorkers="${VITEST_WORKERS:-4}" ;;
    frontend-build) step frontend-build pnpm --dir apps/desktop build ;;
    # The desktop crate's build script checks that every bundled resource
    # exists, target/release/plugin-runtime among them; a fresh checkout has
    # to build it first, as `tauri build` does before bundling.
    plugin-runtime) step plugin-runtime node scripts/build-plugin-runtime.mjs ;;
    *) echo "unknown step $name" >&2; exit 2 ;;
  esac
done
exit $failed
