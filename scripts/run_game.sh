#!/usr/bin/env bash
# Run one decrypted EAPP bundle with the shared fliwheel diagnostics.
# Title-specific entrypoints live in scripts/games/ and pass their key here.

set -euo pipefail

if [[ $# -lt 1 ]]; then
    printf 'usage: %s <game-key> [options] [-- eapp-args...]\n' "$0" >&2
    exit 2
fi

GAME_KEY=$1
shift

case "$GAME_KEY" in
    iquiz)          GAME_TITLE="iQuiz";                BUNDLE_ID="11002"; BUNDLE_VAR="IQUIZ_BUNDLE"; BUNDLE_LOG_VAR="IQUIZ_LOG_DIR"; BUNDLE_CAPTURE_VAR="IQUIZ_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    sat-reading)    GAME_TITLE="SAT Prep Reading";     BUNDLE_ID="11050"; BUNDLE_VAR="SAT_READING_BUNDLE"; BUNDLE_LOG_VAR="SAT_READING_LOG_DIR"; BUNDLE_CAPTURE_VAR="SAT_READING_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    sat-writing)    GAME_TITLE="SAT Prep Writing";     BUNDLE_ID="11051"; BUNDLE_VAR="SAT_WRITING_BUNDLE"; BUNDLE_LOG_VAR="SAT_WRITING_LOG_DIR"; BUNDLE_CAPTURE_VAR="SAT_WRITING_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    sat-math)       GAME_TITLE="SAT Prep Mathematics"; BUNDLE_ID="11052"; BUNDLE_VAR="SAT_MATH_BUNDLE"; BUNDLE_LOG_VAR="SAT_MATH_LOG_DIR"; BUNDLE_CAPTURE_VAR="SAT_MATH_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    vortex)         GAME_TITLE="Vortex";               BUNDLE_ID="12345"; BUNDLE_VAR="VORTEX_BUNDLE"; BUNDLE_LOG_VAR="VORTEX_LOG_DIR"; BUNDLE_CAPTURE_VAR="VORTEX_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    mspacman)       GAME_TITLE="Ms. PAC-MAN";          BUNDLE_ID="14004"; BUNDLE_VAR="MSPACMAN_BUNDLE"; BUNDLE_LOG_VAR="MSPACMAN_LOG_DIR"; BUNDLE_CAPTURE_VAR="MSPACMAN_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    simsbowling)    GAME_TITLE="The Sims Bowling";    BUNDLE_ID="1500C"; BUNDLE_VAR="SIMSBOWLING_BUNDLE"; BUNDLE_LOG_VAR="SIMSBOWLING_LOG_DIR"; BUNDLE_CAPTURE_VAR="SIMSBOWLING_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    simspool)       GAME_TITLE="The Sims Pool";       BUNDLE_ID="1500E"; BUNDLE_VAR="SIMSPOOL_BUNDLE"; BUNDLE_LOG_VAR="SIMSPOOL_LOG_DIR"; BUNDLE_CAPTURE_VAR="SIMSPOOL_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    lost)           GAME_TITLE="LOST";                 BUNDLE_ID="1B200"; BUNDLE_VAR="LOST_BUNDLE"; BUNDLE_LOG_VAR="LOST_LOG_DIR"; BUNDLE_CAPTURE_VAR="LOST_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="lost" ;;
    musika)         GAME_TITLE="musika";               BUNDLE_ID="1C300"; BUNDLE_VAR="MUSIKA_BUNDLE"; BUNDLE_LOG_VAR="MUSIKA_LOG_DIR"; BUNDLE_CAPTURE_VAR="MUSIKA_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    zuma)           GAME_TITLE="Zuma";                 BUNDLE_ID="44444"; BUNDLE_VAR="ZUMA_BUNDLE"; BUNDLE_LOG_VAR="ZUMA_LOG_DIR"; BUNDLE_CAPTURE_VAR="ZUMA_CAPTURE_ROOT"; DEFAULT_VFLIP="auto"; SPECIAL="" ;;
    sudoku)         GAME_TITLE="Sudoku";               BUNDLE_ID="50513"; BUNDLE_VAR="SUDOKU_BUNDLE"; BUNDLE_LOG_VAR="SUDOKU_LOG_DIR"; BUNDLE_CAPTURE_VAR="SUDOKU_CAPTURE_ROOT"; DEFAULT_VFLIP=0; SPECIAL="" ;;
    solitaire)      GAME_TITLE="Royal Solitaire";     BUNDLE_ID="50514"; BUNDLE_VAR="SOLITAIRE_BUNDLE"; BUNDLE_LOG_VAR="SOLITAIRE_LOG_DIR"; BUNDLE_CAPTURE_VAR="SOLITAIRE_CAPTURE_ROOT"; DEFAULT_VFLIP=0; SPECIAL="" ;;
    bejeweled)      GAME_TITLE="Bejeweled";            BUNDLE_ID="55555"; BUNDLE_VAR="BEJEWELED_BUNDLE"; BUNDLE_LOG_VAR="BEJEWELED_LOG_DIR"; BUNDLE_CAPTURE_VAR="BEJEWELED_CAPTURE_ROOT"; DEFAULT_VFLIP="auto"; SPECIAL="" ;;
    tetris)         GAME_TITLE="Tetris";               BUNDLE_ID="66666"; BUNDLE_VAR="TETRIS_BUNDLE"; BUNDLE_LOG_VAR="TETRIS_LOG_DIR"; BUNDLE_CAPTURE_VAR="TETRIS_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    mahjong)        GAME_TITLE="Mahjong";              BUNDLE_ID="77777"; BUNDLE_VAR="MAHJONG_BUNDLE"; BUNDLE_LOG_VAR="MAHJONG_LOG_DIR"; BUNDLE_CAPTURE_VAR="MAHJONG_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    minigolf)       GAME_TITLE="Mini Golf";            BUNDLE_ID="88888"; BUNDLE_VAR="MINIGOLF_BUNDLE"; BUNDLE_LOG_VAR="MINIGOLF_LOG_DIR"; BUNDLE_CAPTURE_VAR="MINIGOLF_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    cubis2)         GAME_TITLE="Cubis 2";              BUNDLE_ID="99999"; BUNDLE_VAR="CUBIS2_BUNDLE"; BUNDLE_LOG_VAR="CUBIS2_LOG_DIR"; BUNDLE_CAPTURE_VAR="CUBIS2_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    pacman)         GAME_TITLE="PAC-MAN";              BUNDLE_ID="AAAAA"; BUNDLE_VAR="PACMAN_BUNDLE"; BUNDLE_LOG_VAR="PACMAN_LOG_DIR"; BUNDLE_CAPTURE_VAR="PACMAN_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    holdem)         GAME_TITLE="Texas Hold'em";       BUNDLE_ID="33333"; BUNDLE_VAR="HOLDEM_BUNDLE"; BUNDLE_LOG_VAR="HOLDEM_LOG_DIR"; BUNDLE_CAPTURE_VAR="HOLDEM_CAPTURE_ROOT"; DEFAULT_VFLIP=1; SPECIAL="" ;;
    *)
        printf 'unknown game key: %s\n' "$GAME_KEY" >&2
        exit 2
        ;;
esac

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
default_bundle="$HOME/Downloads/16-ipod-games/Games_RO/$BUNDLE_ID"
BUNDLE="${!BUNDLE_VAR:-$default_bundle}"
LOG_DIR="${!BUNDLE_LOG_VAR:-/tmp}"
CAPTURE_ROOT="${!BUNDLE_CAPTURE_VAR:-/tmp}"

DO_BUILD=1
DO_CAPTURE=1
DO_HEADLESS=0
DO_VERBOSE=0
TIMEOUT_SECS=0
DUMP_FRAMES=0
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            sed -n '1,5p' "$0"
            printf '\nGame: %s (%s)\n\n' "$GAME_TITLE" "$BUNDLE_ID"
            printf '%s\n' \
                '  --bundle PATH       bundle directory' \
                '  --no-build          skip cargo build' \
                '  --no-capture        skip PPM captures' \
                '  --headless          run without a window' \
                '  --verbose           enable debug logging' \
                '  --timeout SECONDS   terminate after a bounded interval' \
                '  --dump COUNT        dump the first COUNT frames' \
                '  --log-level LEVEL   override RUST_LOG' \
                '  --                  pass remaining arguments to eapp'
            exit 0
            ;;
        --bundle)
            [[ $# -ge 2 ]] || { printf '%s\n' '--bundle requires a path' >&2; exit 2; }
            BUNDLE=$2
            shift 2
            ;;
        --no-build)
            DO_BUILD=0
            shift
            ;;
        --no-capture)
            DO_CAPTURE=0
            shift
            ;;
        --headless)
            DO_HEADLESS=1
            shift
            ;;
        --verbose|--debug)
            DO_VERBOSE=1
            shift
            ;;
        --timeout)
            [[ $# -ge 2 ]] || { printf '%s\n' '--timeout requires seconds' >&2; exit 2; }
            TIMEOUT_SECS=$2
            shift 2
            ;;
        --dump)
            [[ $# -ge 2 ]] || { printf '%s\n' '--dump requires a frame count' >&2; exit 2; }
            DUMP_FRAMES=$2
            shift 2
            ;;
        --log-level)
            [[ $# -ge 2 ]] || { printf '%s\n' '--log-level requires a value' >&2; exit 2; }
            RUST_LOG_OVERRIDE=$2
            shift 2
            ;;
        --)
            shift
            EXTRA_ARGS+=("$@")
            break
            ;;
        -*)
            printf 'unknown launcher option: %s\n' "$1" >&2
            exit 2
            ;;
        *)
            BUNDLE=$1
            shift
            ;;
    esac
done

if [[ ! -d "$BUNDLE" ]]; then
    printf 'bundle directory not found: %s\n' "$BUNDLE" >&2
    printf 'set %s or pass a bundle path\n' "$BUNDLE_VAR" >&2
    exit 1
fi

cd "$repo_dir"

if [[ "$DO_BUILD" -eq 1 ]]; then
    cargo build -p fliwheel-desktop --bin eapp
fi

EAPP_BIN="$(cargo metadata --format-version=1 --no-deps 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug/eapp"
if [[ ! -x "$EAPP_BIN" ]]; then
    printf 'built eapp binary not found: %s\n' "$EAPP_BIN" >&2
    exit 1
fi

if [[ -n "${RUST_LOG_OVERRIDE:-}" ]]; then
    export RUST_LOG="$RUST_LOG_OVERRIDE"
elif [[ "$DO_VERBOSE" -eq 1 ]]; then
    export RUST_LOG='EAPP_GL=debug,EAPP=debug,EAPP_PROGRESS=debug,EAPP_IMPORT=debug'
else
    export RUST_LOG='EAPP_GL=info,EAPP=info,EAPP_PROGRESS=info,EAPP_IMPORT=info'
fi

export CLICKY_EXPERIMENTAL_GL_HLE="${CLICKY_EXPERIMENTAL_GL_HLE:-1}"
export CLICKY_GL_GATE_B="${CLICKY_GL_GATE_B:-1}"
export CLICKY_GL_LIVE_CONTINUOUS="${CLICKY_GL_LIVE_CONTINUOUS:-1}"
if [[ "$DEFAULT_VFLIP" != auto ]]; then
    export CLICKY_GL_PRESENT_VFLIP="${CLICKY_GL_PRESENT_VFLIP:-$DEFAULT_VFLIP}"
fi
export CLICKY_STARTUP_PROGRESS_TRACE="${CLICKY_STARTUP_PROGRESS_TRACE:-1}"
export CLICKY_STARTUP_PROGRESS_FRAMES="${CLICKY_STARTUP_PROGRESS_FRAMES:-300}"
export CLICKY_STARTUP_PROGRESS_INTERVAL="${CLICKY_STARTUP_PROGRESS_INTERVAL:-60}"

if [[ "$SPECIAL" == lost ]]; then
    export CLICKY_EAPP_LOST_SPLASH="${CLICKY_EAPP_LOST_SPLASH:-1}"
fi

stamp=$(date -u +%Y%m%d_%H%M%S)
LOG_FILE="$LOG_DIR/${GAME_KEY}_run_${stamp}.log"
CAPTURE_DIR="$CAPTURE_ROOT/${GAME_KEY}_capture_${stamp}"

if [[ "$DO_CAPTURE" -eq 1 ]]; then
    mkdir -p "$CAPTURE_DIR"
    export CLICKY_STARTUP_CAPTURE_DIR="${CLICKY_STARTUP_CAPTURE_DIR:-$CAPTURE_DIR}"
    export CLICKY_STARTUP_CAPTURE_PERIOD="${CLICKY_STARTUP_CAPTURE_PERIOD:-30}"
    export CLICKY_STARTUP_CAPTURE_MAX_FRAMES="${CLICKY_STARTUP_CAPTURE_MAX_FRAMES:-1500}"
    export CLICKY_STARTUP_CAPTURE_MAX_DUMPS="${CLICKY_STARTUP_CAPTURE_MAX_DUMPS:-500}"
fi

if [[ "$DUMP_FRAMES" -gt 0 ]]; then
    export CLICKY_GL_DUMP_FRAMES="$DUMP_FRAMES"
fi

RUN_ARGS=("$BUNDLE")
if [[ "$DO_HEADLESS" -eq 1 ]]; then
    RUN_ARGS+=(--headless)
fi
if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    RUN_ARGS+=("${EXTRA_ARGS[@]}")
fi

printf 'launching %s\n' "$GAME_TITLE"
printf '  bundle: %s\n' "$BUNDLE"
printf '  binary: %s\n' "$EAPP_BIN"
printf '  log:    %s\n' "$LOG_FILE"
if [[ "$DO_CAPTURE" -eq 1 ]]; then
    printf '  frames: %s\n' "$CAPTURE_DIR"
fi
if [[ "$DO_HEADLESS" -eq 1 ]]; then
    printf '  mode:   headless\n'
else
    printf '  mode:   headed\n'
fi

run_game() {
    if [[ "$TIMEOUT_SECS" -gt 0 ]]; then
        local timeout_cmd="${TIMEOUT_CMD:-}"
        if [[ -z "$timeout_cmd" ]]; then
            if command -v gtimeout >/dev/null 2>&1; then
                timeout_cmd=gtimeout
            elif command -v timeout >/dev/null 2>&1; then
                timeout_cmd=timeout
            else
                "$EAPP_BIN" "${RUN_ARGS[@]}" &
                local pid=$!
                ( sleep "$TIMEOUT_SECS"; kill "$pid" 2>/dev/null || true ) &
                local killer=$!
                wait "$pid"
                local status=$?
                kill "$killer" 2>/dev/null || true
                return "$status"
            fi
        fi
        "$timeout_cmd" "$TIMEOUT_SECS" "$EAPP_BIN" "${RUN_ARGS[@]}"
    else
        "$EAPP_BIN" "${RUN_ARGS[@]}"
    fi
}

set +e
run_game 2>&1 | tee "$LOG_FILE"
status=${PIPESTATUS[0]}
set -e

printf '\n%s finished (exit %s)\n' "$GAME_TITLE" "$status"
printf '  log: %s\n' "$LOG_FILE"
if [[ "$DO_CAPTURE" -eq 1 && -d "$CAPTURE_DIR" ]]; then
    captures=$(find "$CAPTURE_DIR" -name '*.ppm' 2>/dev/null | wc -l | tr -d ' ')
    printf '  captures: %s (%s PPM files)\n' "$CAPTURE_DIR" "$captures"
fi
exit "$status"
