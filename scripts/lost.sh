#!/bin/bash
# Lost (1B200) — Launch with splash screen injection
# The game doesn't render via GL (render server issue), so we inject
# the lostLaunch.raw.lcd5 splash image into the DMA framebuffer.
set -euo pipefail

EAPP_BIN="$(cd "$(dirname "$0")/.." && pwd)/target/release/eapp"
GAME_DIR="/Users/kurt/Downloads/16-ipod-games/Games_RO/1B200"

export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export CLICKY_GL_PRESENT_VFLIP=1
export CLICKY_EAPP_LOST_SPLASH=1

exec "$EAPP_BIN" "$GAME_DIR" "$@"
