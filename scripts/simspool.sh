#!/usr/bin/env bash
#
# scripts/simspool.sh — launch Sims Pool (bundle 1500E) in headed mode.
#
# Usage:
#   ./scripts/simspool.sh                 # default: build + headed run
#   ./scripts/simspool.sh --timeout 15    # auto-terminate after 15 seconds
#   ./scripts/simspool.sh --headless      # no window (for CI / quick checks)
#   ./scripts/simspool.sh --verbose       # debug-level logging
#   ./scripts/simspool.sh --dump 30       # dump first 30 frames as PPM
#
# All CLICKY_* env vars can be overridden from your shell.

set -euo pipefail

GAME_NAME="simspool"
BUNDLE_ID="1500E"
GAME_TITLE="Sims Pool"
DEFAULT_BUNDLE="${SIMSPOOL_BUNDLE:-$HOME/Downloads/16-ipod-games/Games_RO/$BUNDLE_ID}"
BUNDLE="$DEFAULT_BUNDLE"
LOG_DIR="${SIMSPOOL_LOG_DIR:-/tmp}"
CAPTURE_ROOT="${SIMSPOOL_CAPTURE_ROOT:-/tmp}"

DO_BUILD=1
DO_CAPTURE=1
DO_HEADLESS=0
DO_VERBOSE=0
TIMEOUT_SECS=0
DUMP_FRAMES=0
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        --bundle)  BUNDLE="$2"; shift 2 ;;
        --no-build) DO_BUILD=0; shift ;;
        --no-capture) DO_CAPTURE=0; shift ;;
        --headless) DO_HEADLESS=1; shift ;;
        --verbose|--debug) DO_VERBOSE=1; shift ;;
        --timeout) TIMEOUT_SECS="$2"; shift 2 ;;
        --dump) DUMP_FRAMES="$2"; shift 2 ;;
        --log-level) RUST_LOG_OVERRIDE="$2"; shift 2 ;;
        --) shift; EXTRA_ARGS+=("$@"); break ;;
        -*) echo "unknown flag: $1" >&2; exit 2 ;;
        *) BUNDLE="$1"; shift ;;
    esac
done

if [[ ! -d "$BUNDLE" ]]; then
    echo "✗ bundle dir not found: $BUNDLE" >&2
    echo "  set SIMSPOOL_BUNDLE or pass a path" >&2
    exit 1
fi

if [[ "$DO_BUILD" -eq 1 ]]; then
    cargo build -p clicky-desktop --bin eapp 2>&1 | tail -3
fi

EAPP_BIN="$(cargo metadata --format-version=1 --no-deps 2>/dev/null \
    | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["target_directory"])')/debug/eapp"

if [[ -n "${RUST_LOG_OVERRIDE:-}" ]]; then
    export RUST_LOG="$RUST_LOG_OVERRIDE"
elif [[ "$DO_VERBOSE" -eq 1 ]]; then
    export RUST_LOG="EAPP_GL=debug,EAPP=debug,EAPP_IMPORT=debug"
else
    export RUST_LOG="EAPP_GL=info,EAPP=info,EAPP_IMPORT=info"
fi

export CLICKY_EXPERIMENTAL_GL_HLE="${CLICKY_EXPERIMENTAL_GL_HLE:-1}"
export CLICKY_GL_GATE_B="${CLICKY_GL_GATE_B:-1}"
export CLICKY_GL_LIVE_CONTINUOUS="${CLICKY_GL_LIVE_CONTINUOUS:-1}"
export CLICKY_GL_PRESENT_VFLIP="${CLICKY_GL_PRESENT_VFLIP:-1}"
export CLICKY_STARTUP_PROGRESS_TRACE="${CLICKY_STARTUP_PROGRESS_TRACE:-1}"

STAMP="$(date +%Y%m%d_%H%M%S)"
LOG_FILE="$LOG_DIR/${GAME_NAME}_run_${STAMP}.log"
CAPTURE_DIR="$CAPTURE_ROOT/${GAME_NAME}_capture_${STAMP}"

if [[ "$DO_CAPTURE" -eq 1 ]]; then
    mkdir -p "$CAPTURE_DIR"
    export CLICKY_STARTUP_CAPTURE_DIR="${CLICKY_STARTUP_CAPTURE_DIR:-$CAPTURE_DIR}"
    export CLICKY_STARTUP_CAPTURE_PERIOD="${CLICKY_STARTUP_CAPTURE_PERIOD:-30}"
    export CLICKY_STARTUP_CAPTURE_MAX_FRAMES="${CLICKY_STARTUP_CAPTURE_MAX_FRAMES:-1500}"
fi

if [[ "$DUMP_FRAMES" -gt 0 ]]; then
    export CLICKY_GL_DUMP_FRAMES="$DUMP_FRAMES"
fi

RUN_ARGS=("$BUNDLE")
if [[ "$DO_HEADLESS" -eq 1 ]]; then
    RUN_ARGS+=("--headless")
fi
if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    RUN_ARGS+=("${EXTRA_ARGS[@]}")
fi

echo "▸ launching $GAME_TITLE"
echo "  bundle:     $BUNDLE"
echo "  binary:     $EAPP_BIN"
echo "  log:        $LOG_FILE"
echo "  mode:       $([ "$DO_HEADLESS" -eq 1 ] && echo headless || echo headed)"

run_it() {
    if [[ "$TIMEOUT_SECS" -gt 0 ]]; then
        if command -v gtimeout >/dev/null 2>&1; then
            gtimeout "$TIMEOUT_SECS" "$EAPP_BIN" "${RUN_ARGS[@]}"
        elif command -v timeout >/dev/null 2>&1; then
            timeout "$TIMEOUT_SECS" "$EAPP_BIN" "${RUN_ARGS[@]}"
        else
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

set +e
run_it 2>&1 | tee "$LOG_FILE"
EXIT_CODE=${PIPESTATUS[0]}
set -e

echo
echo "──────────────────────────────────────────────────────────"
echo "$GAME_TITLE run finished (exit $EXIT_CODE)"
echo "  log:      $LOG_FILE"
if [[ "$DO_CAPTURE" -eq 1 && -d "$CAPTURE_DIR" ]]; then
    N_CAP="$(find "$CAPTURE_DIR" -name '*.ppm' 2>/dev/null | wc -l | tr -d ' ')"
    echo "  captures: $CAPTURE_DIR/ ($N_CAP PPM files)"
fi
