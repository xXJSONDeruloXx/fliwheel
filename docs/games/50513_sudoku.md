# Sudoku (Bundle 50513)

**Status:** 🟡 MENU PATH VERIFIED | **Draws:** 2 (8s) | **Frames Presented:** 120 | **Engine:** Sudoku/Solitaire (NDC)

## Quick Start
```bash
# Launch with proper env vars (vflip auto-suppressed for this engine)
./target/release/eapp /Users/kurt/Downloads/16-ipod-games/Games_RO/50513

# Exercise the currently verified Menu path in headless mode
CLICKY_EAPP_INPUT_SCRIPT='menu:30-40' \
  CLICKY_EXPERIMENTAL_GL_HLE=1 CLICKY_GL_GATE_B=1 \
  CLICKY_GL_LIVE_CONTINUOUS=1 CLICKY_GL_PRESENT_VFLIP=1 \
  ./target/release/eapp /path/to/Games_RO/50513 --headless
```

## What Renders
- **Splash screen**: 320×240 Rgb565 fullscreen texture, centered and right-side-up
- **Input-wait loop**: Game polls `InputEvents:0` every frame waiting for click wheel
- **Menu transition**: the scripted Menu event reaches the guest and enters its save/settings transition
- The gameplay board and action mapping are not verified yet; a no-input run still remains on the splash/title state

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

## Input status

Sudoku’s `InputEvents:0` import uses the reversed owner-register convention also
seen in the Solitaire family: the event-list owner is in `r5` while `r4` is
empty. The HLE now detects either ABI shape and writes the transition list to
the owner object. With `menu:30-40`, the guest consumes a Menu press at about
frame 433, performs its save-file path, and settles back into a clean idle
state. The host deliberately leaves this event list guest-visible for now;
clearing it immediately caused the guest to miss the event.

See the dated evidence in
[`20260825_sudoku_input.md`](../game_tests/20260825_sudoku_input.md).

The compact scripted event path is proven for the Menu transition, but the
full game-start/action path and raw hardware-packet mapping still need work.

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1    # auto-suppressed for NDC frames
```
