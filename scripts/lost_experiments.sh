#!/bin/bash
# Lost (1B200) Systematic Experiment Runner
set -euo pipefail

EAPP="./target/release/eapp"
GAME="/Users/kurt/Downloads/16-ipod-games/Games_RO/1B200"
TIMEOUT=5
export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export CLICKY_GL_PRESENT_VFLIP=1
export RUST_LOG=EAPP_GL=info,EAPP_IMPORT=info

results_dir="/tmp/lost_experiments"
mkdir -p "$results_dir"

run_test() {
    local name="$1"
    shift
    
    echo "=== TEST: $name ==="
    local logfile="$results_dir/${name}.log"
    
    timeout $TIMEOUT $EAPP $GAME --headless > "$logfile" 2>&1 || true
    
    # Extract key metrics
    local draws=$(grep -c "rasterized" "$logfile" 2>/dev/null || echo 0)
    local last_frame=$(grep "lifecycle frame=" "$logfile" | tail -1 || echo "none")
    local patched=$(grep -c "patched" "$logfile" 2>/dev/null || echo 0)
    local misc6=$(grep "miscTBD:6" "$logfile" | head -1 || echo "none")
    local crashes=$(grep -ciE "crash|panic|fatal|abort" "$logfile" 2>/dev/null || echo 0)
    
    echo "  Draws: $draws | Patched: $patched | Crashes: $crashes"
    echo "  Last frame: $last_frame"
    echo "  miscTBD:6: $misc6"
    echo ""
}

cd /Users/kurt/Developer/clicky

echo "Lost (1B200) Systematic Experiments"
echo "=========================================="
echo ""

# Test 1: Baseline
unset CLICKY_MISCTBD6_RET CLICKY_EAPP_FILL_RSERVER_HEADER CLICKY_EAPP_SKIP_RSERVER
run_test "baseline"

# Test 2: miscTBD:6 returns 1
export CLICKY_MISCTBD6_RET=1
run_test "misctbd6_ret_1"
unset CLICKY_MISCTBD6_RET

# Test 3: miscTBD:6 returns 2
export CLICKY_MISCTBD6_RET=2
run_test "misctbd6_ret_2"
unset CLICKY_MISCTBD6_RET

# Test 4: Fill rserver header with incrementing values
export CLICKY_EAPP_FILL_RSERVER_HEADER=1
run_test "fill_rserver"
unset CLICKY_EAPP_FILL_RSERVER_HEADER

# Test 5: Both fill header AND miscTBD:6=1
export CLICKY_EAPP_FILL_RSERVER_HEADER=1
export CLICKY_MISCTBD6_RET=1
run_test "fill_plus_misctbd6"
unset CLICKY_EAPP_FILL_RSERVER_HEADER CLICKY_MISCTBD6_RET

# Test 6: miscTBD:6 returns large value
export CLICKY_MISCTBD6_RET=9999
run_test "misctbd6_ret_large"
unset CLICKY_MISCTBD6_RET

# Test 7: Skip rserver.bin load
export CLICKY_EAPP_SKIP_RSERVER=1
run_test "skip_rserver"
unset CLICKY_EAPP_SKIP_RSERVER

echo "=========================================="
echo "Full logs at: $results_dir/"
