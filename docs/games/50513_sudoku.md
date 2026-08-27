# Sudoku (Bundle 50513)

**Status:** 🟡 PUZZLE BOARD + INPUT PARTIAL | **Board:** Reached | **Engine:** Sudoku/Solitaire (NDC)

## Quick Start
```bash
# Launch with proper env vars (vflip auto-suppressed for this engine)
./target/release/eapp /Users/kurt/Downloads/16-ipod-games/Games_RO/50513

# Exercise the experimentally verified resource/setup path in headless mode.
# The Sudoku async gates are intentionally opt-in and are not the default
# cross-title contract.
EAPP_SUDOKU_ASYNC0_COMPLETE=1 EAPP_SUDOKU_ASYNC1_COMPLETE=1 \
  EAPP_SUDOKU_ASYNC2_COMPLETE=1 FLIWHEEL_EAPP_ASYNC0_RESULT=length \
  FLIWHEEL_EAPP_INPUT_SCRIPT='action:8200-8201,action:8500-8501,action:9450-9510,wheel=-4:9700-9701,wheel=-4:9750-9751,wheel=-4:9800-9801,action:9900-9901,action:10300-10301' \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 FLIWHEEL_GL_PRESENT_VFLIP=1 \
  ./target/release/eapp /path/to/Games_RO/50513 --headless --cycles 70000000
```

## What Renders
- **Splash screen**: 320×240 Rgb565 fullscreen texture, centered and right-side-up
- **Input-wait loop**: Game polls `InputEvents:0` every frame waiting for click wheel
- **Menu/exit transition**: the scripted Menu event reaches the guest and enters its save/settings teardown loop
- **Player Name**: the complete RLB path now renders the wooden background, ornate
  panel, title, cursor, letter row, and validation instructions coherently
- **Game Setup**: holding Center validates the name and reaches the `Play!`,
  difficulty, and error-checking screen
- **Tutorial**: completing the name entry and selecting `Play!` reaches the
  built-in tutorial screen
- **Puzzle board**: dismissing the tutorial reaches a populated 9×9 board with
  the side controls, a visible cursor, and the numbered entry palette; the
  latest wheel retest leaves the palette on `1` while the filtered event still
  reaches the guest dispatcher

## Bundle Info
- **Executable:** `Sudoku_1_1_2703081.bin` (eapp format)
- **Save File:** `savefile.dat` (loaded as 0 bytes if missing)

## Engine Characteristics
- **NDC coordinates**: Vertex positions in 0–1 range (not pixel-space 0–320)
- **No ordinal-158**: Frame begin is implicit; per-frame loop is 159→149→157
- **Top-to-bottom rendering**: Vflip must be suppressed (auto-detected via `ndc_frame` flag)
- **RLB resource table**: 236 entries; late menu assets are read through
  absolute seek positions in `Sudoku.rlb`

## Emulator Fixes Required
1. ✅ Auto-begin on present (no ordinal-158 begin)
2. ✅ NDC-to-pixel viewport scaling (max_coord < 2.0 detection)
3. ✅ Vflip suppression for NDC frames
4. ✅ 0-draw frame preservation (idle input-wait loop)
5. ✅ Title-scoped RLB seek/read completion probe (opt-in)
6. ✅ Half-texel UV containment for centered atlas/full-surface edges
7. ✅ Name → setup → tutorial → populated puzzle-board transition (opt-in)

## Input status

Sudoku’s `InputEvents:0` import uses the reversed owner-register convention also
seen in the Solitaire family: the event-list owner is in `r5` while `r4` is
empty. The HLE now detects either ABI shape and writes the transition list to
the owner object. For the reversed shape, the HLE defers clearing the consumed
head until the next poll and remembers the owner when the wrapper temporarily
drops `r5`, so later press/release edges remain guest-visible without becoming
stale. With `menu:30-40`, the guest consumes a Menu press at about frame 433,
performs its save-file path, and settles back into a clean idle state.

Static disassembly identifies that Menu edge as a teardown/exit transition,
not the puzzle-start control: it clears the title runtime object and leaves
the guest alternating its save/settings states while waiting for that object.
The current title-scoped resource probe stages the complete RLB, honors the
guest's second seek (`0x8d381`) before its 153,884-byte payload read, and
executes the callback chain. The verified interactive route is now `PLAYER
NAME` → `GAME SETUP` → `TUTORIAL` → populated puzzle board. The board cursor and
number palette render. The latest retest produces the expected filtered wheel
event, but the board-state listener chain lacks the selector object that is
present during name entry, so the visible palette remains on `1`. A legal
user-entered digit, full cursor movement, pen mode, audio, and save behavior
remain unverified. The evidence is recorded in
[`20260827_sudoku_rlb_seek_and_setup.md`](../game_tests/20260827_sudoku_rlb_seek_and_setup.md)
and [`20260827_sudoku_puzzle_board_and_input.md`](../game_tests/20260827_sudoku_puzzle_board_and_input.md).

See the dated evidence in
[`20260825_sudoku_input.md`](../game_tests/20260825_sudoku_input.md).

The compact scripted event path is proven for the Menu transition and the
experimental name/setup/tutorial/board path. The default resource completion
contract, raw hardware-packet mapping, legal cell entry, sound, and persistence
still need work.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1    # auto-suppressed for NDC frames

# Optional Sudoku resource completion experiment:
EAPP_SUDOKU_ASYNC0_COMPLETE=1
EAPP_SUDOKU_ASYNC1_COMPLETE=1
EAPP_SUDOKU_ASYNC2_COMPLETE=1
FLIWHEEL_EAPP_ASYNC0_RESULT=length
```
