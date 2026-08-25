#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
BUNDLE_DIR="${SOLITAIRE_BUNDLE:-/Users/kurt/Downloads/16-ipod-games/Games_RO/50514}"
export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export CLICKY_GL_PRESENT_VFLIP=0  # Solitaire uses NDC coords, no vflip
exec ./target/release/eapp "$BUNDLE_DIR" "$@"
