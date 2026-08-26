# Ms. Pac-Man (Bundle 14004)

**Status:** ⚠️ LOADING SCREEN ONLY | **Evidence:** scripted probe reaches Namco loading art with texture/text artifacts | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/games/mspacman.sh
./scripts/games/mspacman.sh --timeout 15
./scripts/games/mspacman.sh --headless
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
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
