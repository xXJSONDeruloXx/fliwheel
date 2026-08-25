# Sudoku (Bundle 50513)

**Status:** ✅ WORKS | **Draws:** 2 (8s) | **Frames Presented:** 120 | **Engine:** Sudoku/Solitaire (NDC)

## Quick Start
```bash
# Launch with proper env vars (vflip auto-suppressed for this engine)
./target/release/eapp /Users/kurt/Downloads/16-ipod-games/Games_RO/50513
```

## What Renders
- **Splash screen**: 320×240 Rgb565 fullscreen texture, centered and right-side-up
- **Input-wait loop**: Game polls `InputEvents:0` every frame waiting for click wheel
- No keyboard input injection yet, so the game stays on splash in headless mode

## Bundle Info
- **Executable:** `Sudoku_1_1_2703081.bin` (eapp format)
- **Save File:** `savefile.dat` (loaded as 0 bytes if missing)

## Engine Characteristics
- **NDC coordinates**: Vertex positions in 0–1 range (not pixel-space 0–320)
- **No ordinal-158**: Frame begin is implicit; per-frame loop is 159→149→157
- **Top-to-bottom rendering**: Vflip must be suppressed (auto-detected via `ndc_frame` flag)
- Minimal asset footprint — game logic is all in code, single splash texture

## Emulator Fixes Required
1. ✅ Auto-begin on present (no ordinal-158 begin)
2. ✅ NDC-to-pixel viewport scaling (max_coord < 2.0 detection)
3. ✅ Vflip suppression for NDC frames
4. ✅ 0-draw frame preservation (idle input-wait loop)

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1    # auto-suppressed for NDC frames
```
