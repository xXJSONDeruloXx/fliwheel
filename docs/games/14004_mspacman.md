# Ms. Pac-Man (Bundle 14004)

**Status:** ✅ WORKS | **Draws:** 26,405 (10s) | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/mspacman.sh
./scripts/mspacman.sh --timeout 15
./scripts/mspacman.sh --headless
```

## Bundle Info
- **Executable:** `mspacman_1_1_2805293.bin` (eapp format)
- **Asset Format:** `.wav` (20 files) — no .pix or .ipd, uses built-in textures

## Assets
- **Audio:** 20 `.wav` files for game sounds (coin, die, eat ghost, fruit bounce, etc.)
- **No external textures** — all graphics are generated procedurally or from code

## Notable
- Classic arcade game with simple but recognizable graphics
- One of the few games with purely `.wav` audio assets
- No .pix/.ipd texture files — all rendering from embedded data

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
