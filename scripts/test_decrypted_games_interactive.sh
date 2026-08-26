#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
    printf 'usage: %s /path/to/Games_RO [bundle-id ...]\n' "$0" >&2
    exit 2
fi

games_root=$1
shift
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
eapp=${EAPP:-"$repo_dir/target/release/eapp"}
cycles=${CYCLES:-50000000}
input_script=${INPUT_SCRIPT:-'action:18-20,action:45-47,action:80-82,action:130-132,wheel=3:180-182,left:230-232,right:280-282,up:330-332,down:380-382,action:430-432'}
stamp=$(date -u +%Y%m%d_%H%M%S)
run_root=${RUN_ROOT:-"/tmp/fliwheel_interactive_$stamp"}
report="$run_root/interactive_matrix.md"
logs="$run_root/logs"
captures="$run_root/captures"

if [[ ! -d "$games_root" ]]; then
    printf 'Games_RO directory does not exist: %s\n' "$games_root" >&2
    exit 2
fi

if [[ ! -x "$eapp" ]]; then
    cargo build --release -p fliwheel-desktop --bin eapp
fi

mkdir -p "$logs" "$captures"

cat > "$report" <<EOF
# Decrypted-game interactive HLE run

Date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')  
Games: \`$games_root\`  
EAPP: \`$eapp\`  
Cycles: $cycles per bundle  
Input script: \`$input_script\`  
Audio: guest events traced; headless host sink disabled  

This is an automated interaction probe, not a claim of full game completion.
The captures and logs are the evidence for the per-title assessment.

| Bundle | Title | Exit | Last frame | Unique hashes | Hash changes | Captures | Max draws | Zero-draw frames | Audio events | Fatal signatures | Log | Captures |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
EOF

title_for_bundle() {
    python3 - "$1/Manifest.plist" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as stream:
    manifest = plistlib.load(stream)
print(manifest.get("Name") or "<unknown>")
PY
}

selected_bundle() {
    local id=$1
    if [[ $# -eq 1 ]]; then
        return 0
    fi
    local wanted
    for wanted in "${@:2}"; do
        if [[ "$wanted" == "$id" ]]; then
            return 0
        fi
    done
    return 1
}

export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export CLICKY_GL_PRESENT_VFLIP=1
export CLICKY_EAPP_INPUT_SCRIPT="$input_script"
export EAPP_AUDIO_DISABLE=1
export EAPP_AUDIO_EVENT_TRACE=1
export RUST_LOG=${RUST_LOG:-'EAPP_GL=info,EAPP=warn,EAPP_IMPORT=info,EAPP_AUDIO=info,EAPP_PROGRESS=info'}

for bundle in "$games_root"/*; do
    [[ -d "$bundle" ]] || continue
    id=$(basename "$bundle")
    selected_bundle "$id" "$@" || continue

    title=$(title_for_bundle "$bundle")
    log="$logs/$id.log"
    capture_dir="$captures/$id"
    mkdir -p "$capture_dir"

    run_env=(
        "CLICKY_STARTUP_CAPTURE_DIR=$capture_dir"
        "CLICKY_STARTUP_CAPTURE_PERIOD=1"
        "CLICKY_STARTUP_CAPTURE_MAX_FRAMES=700"
        "CLICKY_STARTUP_CAPTURE_MAX_DUMPS=100"
    )
    if [[ "$id" == "66666" ]]; then
        # The parsed-resource completion experiment is currently Tetris-only.
        # Other bundles use different callback/resource contracts; enabling it
        # globally turns valid title-specific behavior into false faults.
        run_env+=("CLICKY_EAPP_ASYNC3_COMPLETE=1")
    fi

    set +e
    env "${run_env[@]}" "$eapp" "$bundle" --headless --cycles "$cycles" >"$log" 2>&1
    exit_code=$?
    set -e

    metrics=$(awk -F '\\t' 'NR > 1 { rows++; hashes[$8] = 1; if ($9 == "hash_change") changes++; if ($5 > max) max = $5; if ($5 == 0) zero++ } END { printf "%d %d %d %d %d", rows, length(hashes), changes, max + 0, zero + 0 }' "$capture_dir/manifest.tsv" 2>/dev/null || true)
    if [[ -z "$metrics" ]]; then
        metrics="0 0 0 0 0"
    fi
    read -r last_frame unique_hashes hash_changes max_draws zero_draws <<<"$metrics"
    captures_written=$(find "$capture_dir" -maxdepth 1 -type f -name '*.ppm' | wc -l | tr -d ' ')
    audio_events=$(rg -c 'AudioEvent' "$log" || true)
    fatal_signatures=$(rg -c 'fatal eapp error|panicked at|MemoryFault|unmapped import|unsupported .*ordinal' "$log" || true)
    : "${last_frame:=0}"
    : "${unique_hashes:=0}"
    : "${hash_changes:=0}"
    : "${max_draws:=0}"
    : "${zero_draws:=0}"
    : "${audio_events:=0}"
    : "${fatal_signatures:=0}"

    log_link="${log#$repo_dir/}"
    printf '| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | [%s](%s) | [%s](%s) |\n' \
        "$id" "$title" "$exit_code" "$last_frame" "$unique_hashes" \
        "$hash_changes" "$captures_written" "$max_draws" "$zero_draws" \
        "$audio_events" "$fatal_signatures" "$log_link" "$log" \
        "$id captures" "$capture_dir" >> "$report"
done

cat >> "$report" <<'EOF'

Exit code `0` means the requested CPU-cycle budget completed; a nonzero code
needs log inspection. A nonzero audio-event count means the guest reached the
currently recognized resource-indexed sound consumer, not that physical output
or mixer parity is already verified. Hash changes show visual state
transitions only; they do not establish that a title is playable through its
content.
EOF

printf 'report: %s\n' "$report"
printf 'logs:   %s\n' "$logs"
printf 'captures: %s\n' "$captures"
