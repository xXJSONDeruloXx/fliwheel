#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    printf 'usage: %s /path/to/Games_RO\n' "$0" >&2
    exit 2
fi

games_root=$1
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
eapp=${EAPP:-"$repo_dir/target/release/eapp"}
timeout_seconds=${TIMEOUT_SECONDS:-8}
report_dir=${REPORT_DIR:-"$repo_dir/docs/game_tests"}
timestamp=$(date +%Y%m%d_%H%M%S)
report="$report_dir/${timestamp}_decrypted_games.md"
logs="$report_dir/${timestamp}_logs"

if [[ ! -d "$games_root" ]]; then
    printf 'Games_RO directory does not exist: %s\n' "$games_root" >&2
    exit 2
fi

if [[ ! -x "$eapp" ]]; then
    cargo build --release -p fliwheel-desktop --bin eapp
fi

timeout_cmd=${TIMEOUT_CMD:-}
if [[ -z "$timeout_cmd" ]]; then
    if command -v gtimeout >/dev/null 2>&1; then
        timeout_cmd=gtimeout
    elif command -v timeout >/dev/null 2>&1; then
        timeout_cmd=timeout
    else
        printf 'need gtimeout or timeout (set TIMEOUT_CMD to override)\n' >&2
        exit 2
    fi
fi

mkdir -p "$report_dir" "$logs"

cat > "$report" <<EOF
# Decrypted-game HLE run

Date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')  
Games: \`$games_root\`  
EAPP: \`$eapp\`  
Timeout: ${timeout_seconds}s per bundle  
Input: none

| Bundle | Exit | Last frame | Last draws | Rasterized draws | Skipped draws |
| --- | ---: | ---: | ---: | ---: | ---: |
EOF

export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export RUST_LOG=${RUST_LOG:-'EAPP_GL=info,EAPP=warn,EAPP_IMPORT=info,EAPP_HW=info'}

for bundle in "$games_root"/*; do
    [[ -d "$bundle" ]] || continue
    id=$(basename "$bundle")
    log="$logs/$id.log"

    set +e
    "$timeout_cmd" "$timeout_seconds" "$eapp" "$bundle" --headless >"$log" 2>&1
    exit_code=$?
    set -e

    last_frame=$(sed -n 's/.*lifecycle frame=\([0-9][0-9]*\) draws=.*/\1/p' "$log" | tail -1)
    last_draws=$(sed -n 's/.*lifecycle frame=[0-9][0-9]* draws=\([0-9][0-9]*\).*/\1/p' "$log" | tail -1)
    rasterized=$(grep -Ec 'draw[0-9]+ rasterized' "$log" || true)
    skipped=$(grep -Ec 'draw[0-9]+ skipped' "$log" || true)

    : "${last_frame:=0}"
    : "${last_draws:=0}"
    printf '| %s | %s | %s | %s | %s | %s |\n' \
        "$id" "$exit_code" "$last_frame" "$last_draws" "$rasterized" "$skipped" >> "$report"
done

cat >> "$report" <<'EOF'

The watchdog exit is normally `124`; inspect the per-game logs before treating
any nonzero exit as a runtime failure.
EOF

printf '%s\n' "report: $report"
printf '%s\n' "logs:   $logs"
