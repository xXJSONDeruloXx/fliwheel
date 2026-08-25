# Tetris (Bundle 66666)

**Status:** ✅ WORKS | **Draws:** 10,572 (8s) | **Engine:** Tetris Runtime (reference)

## Quick Start
```bash
./scripts/tetris.sh                 # default: build + headed run
./scripts/tetris.sh --timeout 15    # auto-terminate after 15 seconds
./scripts/tetris.sh --headless      # no window
./scripts/tetris.sh --dump 100      # dump first 100 frames as PPM
./scripts/tetris.sh --verbose       # debug-level logging
```

## Bundle Info
- **Executable:** `Tetris_1_1_2563292.bin` (eapp format, 40-byte header)
- **Entry Point:** `0x1802_22a4` (main per-frame function)
- **Asset Format:** `.pix` (38 files) — the most of any game

## Assets
- **Background:** `screenBG_565.pix` (320×240, RGB565)
- **Logo:** `tetrisLogoT_4444.pix` (250×162, RGBA4444), `eaLogo_5551.pix` (50×50, RGBA5551)
- **Fonts:** `f8x10`, `f10x12`, `f13x13menu`, `f16x16menu`, `f17x16game`, `f23x22game` (all A8 alpha atlases)
- **UI:** `arrows_a8.pix`, `battery_5551.pix`, `battery_8888.pix`
- **Audio:** 11 `.wav` files (Clear, Drop, Hold, LevelUp, Line, Lock, Move, Rotate, Score, Tetris, Touch)

## Save Files
Loaded from `.clicky-saves/` in the bundle directory:
- `game.sav` (3,561 bytes) — MGCT header: score, level, lines, board state
- `prefs.sav` (127 bytes) — RPCT header: settings/preferences

Save files from physical iPod are stored in `data/tetris_saves/`.

## Texture Details
| File | Format | Dimensions | Notes |
|------|--------|------------|-------|
| screenBG_565.pix | RGB565 | 320×240 | Full-screen background |
| tetrisLogoT_4444.pix | RGBA4444 | 250×162 | Title logo |
| eaLogo_5551.pix | RGBA5551 | 50×50 | EA logo sprite |
| f8x10text{1-3}_a8.pix | A8 | 784×20 | 8×10 font atlas (3 layers) |
| f13x13menu{1-3}_a8.pix | A8 | varies | 13×13 menu font (3 layers) |
| f16x16menu{1-3}_a8.pix | A8 | varies | 16×16 menu font (3 layers) |
| arrows_a8.pix | A8 | varies | Scroll arrows |

## Controls
| Key | Action |
|-----|--------|
| ↑ | Move piece up / navigate |
| ↓ | Move piece down |
| ← | Move piece left |
| → | Move piece right |
| Enter | Action / Select |
| M | Menu / Back |

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
CLICKY_STARTUP_PROGRESS_TRACE=1
```

## Testing Notes
- Best-tested game in the emulator (reference implementation)
- GL trace fixture: `clicky-core/tests/fixtures/eapp/tetris_gl_trace.json`
- Tetris-specific code paths in `eapp/mod.rs` gated by bundle ID "66666"
- Frame capture produces PPM files visible with `open /tmp/tetris_capture_*.ppm`
