#!/usr/bin/env bash
#
# scripts/tetris.sh — launch the Bejeweled iPod game in headed mode with all the
# experimental live GL HLE diagnostics enabled.
#
# Usage:
#   ./scripts/tetris.sh                 # default: build + headed run, log to /tmp
#   ./scripts/tetris.sh --timeout 15    # auto-terminate after 15 seconds
#   ./scripts/tetris.sh --no-capture    # skip PPM frame captures
#   ./scripts/tetris.sh --no-build      # skip the cargo build step
#   ./scripts/tetris.sh --headless      # no window (for CI / quick checks)
#   ./scripts/tetris.sh --verbose       # debug-level logging
#   ./scripts/tetris.sh --dump 30       # dump first 30 presented frames as PPM
#   ./scripts/tetris.sh -- /path/to/bundle   # override bundle dir (must be last)
#
# Logs are written to: /tmp/tetris_run_YYYYMMDD_HHMMSS.log
# Captures (if enabled): /tmp/tetris_capture_YYYYMMDD_HHMMSS/
#
# All CLICKY_* env vars below can be overridden from your shell.

set -euo pipefail

# ---------------------------------------------------------------------------
# Defaults — tweak these or override via env / flags
# ---------------------------------------------------------------------------

# Path to the Bejeweled bundle (Games_RO/55555). Override with $TETRIS_BUNDLE.
DEFAULT_BUNDLE="${TETRIS_BUNDLE:-$HOME/Downloads/16-ipod-games/Games_RO/55555}"
BUNDLE="$DEFAULT_BUNDLE"

# Where to put logs and captures.
LOG_DIR="${TETRIS_LOG_DIR:-/tmp}"
CAPTURE_ROOT="${TETRIS_CAPTURE_ROOT:-/tmp}"

# Live GL HLE flags (the core experimental renderer).
DO_BUILD=1
DO_CAPTURE=1
DO_HEADLESS=0
DO_VERBOSE=0
TIMEOUT_SECS=0        # 0 = run until window closed / Ctrl-C
DUMP_FRAMES=0         # 0 = no per-frame PPM dump
EXTRA_ARGS=()

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        --bundle)
            BUNDLE="$2"; shift 2 ;;
        --no-build)
            DO_BUILD=0; shift ;;
        --no-capture)
            DO_CAPTURE=0; shift ;;
        --headless)
            DO_HEADLESS=1; shift ;;
        --verbose|--debug)
            DO_VERBOSE=1; shift ;;
        --timeout)
            TIMEOUT_SECS="$2"; shift 2 ;;
        --dump)
            DUMP_FRAMES="$2"; shift 2 ;;
        --log-level)
            RUST_LOG_OVERRIDE="$2"; shift 2 ;;
        --)
            shift; EXTRA_ARGS+=("$@"); break ;;
        -*)
            echo "unknown flag: $1" >&2; exit 2 ;;
        *)
            # First positional = bundle dir; rest passed through.
            if [[ "$BUNDLE" == "$DEFAULT_BUNDLE" && -z "${TETRIS_BUNDLE:-}" ]]; then
                BUNDLE="$1"; shift
            else
                EXTRA_ARGS+=("$1"); shift
            fi
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

if [[ ! -d "$BUNDLE" ]]; then
    echo "✗ bundle dir not found: $BUNDLE" >&2
    echo "  set TETRIS_BUNDLE or pass a path: ./scripts/tetris.sh /path/to/55555" >&2
    exit 1
fi

# Find a cargo/rustc. Prefer rustup-managed stable toolchain, fall back to PATH.
TOOLCHAIN_DIR="$(rustc --print sysroot 2>/dev/null || true)"
if [[ -z "$TOOLCHAIN_DIR" ]]; then
    RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
    if [[ -d "$RUSTUP_HOME/toolchains/stable-aarch64-apple-darwin" ]]; then
        TOOLCHAIN_DIR="$RUSTUP_HOME/toolchains/stable-aarch64-apple-darwin"
    fi
fi
if [[ -n "$TOOLCHAIN_DIR" ]]; then
    export PATH="$TOOLCHAIN_DIR/bin:$PATH"
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "✗ cargo not found on PATH (and no rustup toolchain detected)" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Timestamped output locations
# ---------------------------------------------------------------------------

STAMP="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/tetris_run_${STAMP}.log"
CAPTURE_DIR="$CAPTURE_ROOT/tetris_capture_${STAMP}"

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

if [[ "$DO_BUILD" -eq 1 ]]; then
    echo "▸ building clicky-desktop (eapp)..."
    # Capture build output; only show on failure (keeps the launch output clean).
    BUILD_LOG="$(mktemp -t tetris_build.XXXXXX)"
    if ! cargo build -p clicky-desktop --bin eapp >"$BUILD_LOG" 2>&1; then
        echo "✗ build failed:" >&2
        cat "$BUILD_LOG" >&2
        rm -f "$BUILD_LOG"
        exit 1
    fi
    rm -f "$BUILD_LOG"
fi

EAPP_BIN="$(cargo metadata --format-version=1 --no-deps 2>/dev/null \
    | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["target_directory"])')/debug/eapp"
if [[ ! -x "$EAPP_BIN" ]]; then
    echo "✗ built binary not found at expected path: $EAPP_BIN" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Compose env for the experimental live GL HLE path
# ---------------------------------------------------------------------------

# Log level: default info on the GL + EAPP targets; --verbose bumps to debug.
if [[ -n "${RUST_LOG_OVERRIDE:-}" ]]; then
    export RUST_LOG="$RUST_LOG_OVERRIDE"
elif [[ "$DO_VERBOSE" -eq 1 ]]; then
    export RUST_LOG="EAPP_GL=debug,EAPP=debug,EAPP_PROGRESS=debug,EAPP_IMPORT=debug"
else
    export RUST_LOG="EAPP_GL=info,EAPP=info,EAPP_PROGRESS=info,EAPP_IMPORT=info"
fi

export CLICKY_EXPERIMENTAL_GL_HLE="${CLICKY_EXPERIMENTAL_GL_HLE:-1}"
export CLICKY_GL_GATE_B="${CLICKY_GL_GATE_B:-1}"
export CLICKY_GL_LIVE_CONTINUOUS="${CLICKY_GL_LIVE_CONTINUOUS:-1}"
export CLICKY_GL_PRESENT_VFLIP="${CLICKY_GL_PRESENT_VFLIP:-1}"

# Startup progress trace — shows frame lifecycle, splash phases, time API.
export CLICKY_STARTUP_PROGRESS_TRACE="${CLICKY_STARTUP_PROGRESS_TRACE:-1}"
export CLICKY_STARTUP_PROGRESS_FRAMES="${CLICKY_STARTUP_PROGRESS_FRAMES:-300}"
export CLICKY_STARTUP_PROGRESS_INTERVAL="${CLICKY_STARTUP_PROGRESS_INTERVAL:-60}"

# Optional frame captures (PPM). Disabled with --no-capture.
if [[ "$DO_CAPTURE" -eq 1 ]]; then
    mkdir -p "$CAPTURE_DIR"
    export CLICKY_STARTUP_CAPTURE_DIR="${CLICKY_STARTUP_CAPTURE_DIR:-$CAPTURE_DIR}"
    export CLICKY_STARTUP_CAPTURE_PERIOD="${CLICKY_STARTUP_CAPTURE_PERIOD:-30}"
    export CLICKY_STARTUP_CAPTURE_MAX_FRAMES="${CLICKY_STARTUP_CAPTURE_MAX_FRAMES:-1500}"
    export CLICKY_STARTUP_CAPTURE_MAX_DUMPS="${CLICKY_STARTUP_CAPTURE_MAX_DUMPS:-500}"
fi

# Optional first-N-frames dump (distinct from hash-change captures).
if [[ "$DUMP_FRAMES" -gt 0 ]]; then
    export CLICKY_GL_DUMP_FRAMES="$DUMP_FRAMES"
fi

# ---------------------------------------------------------------------------
# Launch
# ---------------------------------------------------------------------------

RUN_ARGS=("$BUNDLE")
if [[ "$DO_HEADLESS" -eq 1 ]]; then
    RUN_ARGS+=("--headless")
fi
# Guard against empty EXTRA_ARGS under `set -u`.
if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    RUN_ARGS+=("${EXTRA_ARGS[@]}")
fi

echo "▸ launching Bejeweled"
echo "  bundle:     $BUNDLE"
echo "  binary:     $EAPP_BIN"
echo "  log:        $LOG_FILE"
if [[ "$DO_CAPTURE" -eq 1 ]]; then
    echo "  captures:   $CAPTURE_DIR/"
fi
if [[ "$TIMEOUT_SECS" -gt 0 ]]; then
    echo "  timeout:    ${TIMEOUT_SECS}s (auto-terminate)"
fi
if [[ "$DO_HEADLESS" -eq 1 ]]; then
    echo "  mode:       headless"
else
    echo "  mode:       headed (close window or Ctrl-C to stop)"
fi
echo "  RUST_LOG:   $RUST_LOG"
echo

# We tee to both the terminal and the log file. When a timeout is requested we
# wrap in `timeout` (BSD/GNU compatible: `timeout` on Linux, fallback below).
run_it() {
    if [[ "$TIMEOUT_SECS" -gt 0 ]]; then
        if command -v gtimeout >/dev/null 2>&1; then
            gtimeout "$TIMEOUT_SECS" "$EAPP_BIN" "${RUN_ARGS[@]}"
        elif command -v timeout >/dev/null 2>&1; then
            timeout "$TIMEOUT_SECS" "$EAPP_BIN" "${RUN_ARGS[@]}"
        else
            # No `timeout` binary — run in background and kill after N seconds.
            "$EAPP_BIN" "${RUN_ARGS[@]}" & local pid=$!
            ( sleep "$TIMEOUT_SECS"; kill "$pid" 2>/dev/null || true ) &
            local killer=$!
            wait "$pid" 2>/dev/null || true
            kill "$killer" 2>/dev/null || true
        fi
    else
        "$EAPP_BIN" "${RUN_ARGS[@]}"
    fi
}

# Run, capturing all output. Don't let set -e kill us on non-zero exit (the
# process is normally killed by timeout / window-close / Ctrl-C).
set +e
run_it 2>&1 | tee "$LOG_FILE"
EXIT_CODE=${PIPESTATUS[0]}
set -e

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

echo
echo "──────────────────────────────────────────────────────────"
echo "run finished (exit $EXIT_CODE)"
echo "  log:      $LOG_FILE"
if [[ "$DO_CAPTURE" -eq 1 && -d "$CAPTURE_DIR" ]]; then
    N_CAP="$(find "$CAPTURE_DIR" -name '*.ppm' 2>/dev/null | wc -l | tr -d ' ')"
    echo "  captures: $CAPTURE_DIR/ ($N_CAP PPM files)"
fi

# Quick diagnostic counts from the log, if present.
if [[ -s "$LOG_FILE" ]]; then
    N_DRAWS="$(grep -c 'rasterized' "$LOG_FILE" 2>/dev/null || echo 0)"
    N_SKIPS="$(grep -c 'skipped:' "$LOG_FILE" 2>/dev/null || echo 0)"
    N_FRAMES="$(grep -c 'frame_diag ' "$LOG_FILE" 2>/dev/null || echo 0)"
    echo "  draws rasterized (log lines): $N_DRAWS"
    echo "  draws skipped (log lines):    $N_SKIPS"
    echo "  frame diagnostics:            $N_FRAMES"
    echo
    echo "  tip: view captures with:  open $CAPTURE_DIR/*.ppm   (macOS)"
    echo "  tip: tail the log:        tail -f $LOG_FILE"
fi
