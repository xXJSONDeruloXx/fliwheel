# Tetris (Bundle 66666)

**Status:** 🟡 CENTERED BOARD + INPUT PATH VERIFIED | **Gameplay:** gravity, controls, and hard drop smoke verified; parity open | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/games/tetris.sh                 # default: build + headed run
./scripts/games/tetris.sh --timeout 15    # auto-terminate after 15 seconds
./scripts/games/tetris.sh --headless      # no window
./scripts/games/tetris.sh --dump 100      # dump first 100 frames as PPM
./scripts/games/tetris.sh --verbose       # debug-level logging
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
- **Audio:** 11 `.wav` files are staged during parsed boot; gameplay resource
  events resolve to `Menu.wav`, `Move.wav`, and `Drop.wav`, and the headed EAPP
  frontend now routes supported assets to its host sink. Physical-output and
  mixing parity remain unverified.

## Save Files
The current local `66666` bundle contains zero-byte legacy
`.clicky-saves/game.sav` and `.clicky-saves/prefs.sav` placeholders. New
fliwheel runs use `.fliwheel-saves`; the runtime still reads the legacy files.
The 3,561-byte `MGCT` and
127-byte `RPCT` files described in [`TETRIS_SAVE_FORMAT.md`](../TETRIS_SAVE_FORMAT.md)
come from an older physical-iPod extraction that is not present in the current
workspace; that document is retained as an unverified reverse-engineering lead,
not as a current emulator fixture.

## Current evidence

The live renderer now produces readable credits and the real first-run
`PLAYER NAME / ENTER YOUR NAME` screen. Positive wheel input moves the selected
character and Select commits it to the name field. The scripted path reaches
`MENU`, `PICK GAME`, `CONTROLS`, and the initial 10×20 board. Tetris now honors
the guest's explicit color clear, retains the framebuffer during incremental
board updates, and carries the paired tile transforms for the matrix/mino
materials. Full gameplay frames now restore the board origin from the
`matrix_565.pix` draw, centering the 115×223 well at `(102,7)` and placing the
first active cell inside it. The board remains intact and input changes the
dynamic draw stream.
A focused follow-up now verifies pause/resume, gravity, side-button response,
the hard-drop transition, and a 1,000-frame repeat-drop/game-over regression
reaches `Drop.wav`, `Lock.wav`, and `GameOver.wav` resource events. Wheel
displacement, later piece/line-clear transforms, exact physical mapping,
persistence, host audio output, and full long-run parity are still open.

The old no-input probe used a diagnostic `FLIWHEEL_EAPP_HOST_EVENT_FLAGS=0x10`
injection and is not evidence that ordinary no-input execution reaches the
same state transition.

See [`20260825_tetris_text_and_name_entry.md`](../game_tests/20260825_tetris_text_and_name_entry.md)
for the exact commands and capture artifacts.
See [`20260826_tetris_gameplay_controls.md`](../game_tests/20260826_tetris_gameplay_controls.md)
for the gameplay timing and control evidence.
See [`20260826_tetris_board_origin.md`](../game_tests/20260826_tetris_board_origin.md)
for the centered-board fix and long-run renderer checkpoint.
See [`20260826_tetris_incremental_transform_regression.md`](../game_tests/20260826_tetris_incremental_transform_regression.md)
for the byte-identical A/B receipt covering the full-to-incremental draw
transition.

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

## Controls and reference behavior

The emulator's scripted harness can inject `wheel`, `left`, `right`, `down`,
and `action` packets at selected guest frames. The current smoke path verifies
gravity, pause/resume, side-button response, and hard drop; exact gameplay
mapping is still being checked against device behavior. The contemporary [iLounge review](https://www.ilounge.com/index.php/reviews/entry/electronic-arts-tetris)
describes wheel sweeps for horizontal movement, clickwheel side buttons for
rotation, and center/down actions for faster or instant drops.

| Key | Action |
|-----|--------|
| Wheel | Navigate menus/name entry; gameplay displacement under verification |
| Left / Right | Responsive gameplay path; exact rotate/move labels under verification |
| Down | Hard-drop/lock transition observed |
| Enter / Action | Select, commit, pause, resume, or advance the current scene |
| M | Menu / Back |

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
FLIWHEEL_STARTUP_PROGRESS_TRACE=1
```

Tetris framebuffer retention and incremental tile-transform handling are now
automatic for bundle `66666`; no preservation override is required.

## Testing Notes
- Best-tested game in the emulator and current renderer reference
- GL trace fixture: `core/tests/fixtures/eapp/tetris_gl_trace.json`
- Tetris-specific code paths in `eapp/mod.rs` gated by bundle ID "66666"
- Clickwheel ingress evidence: [`20260825_clickwheel_input.md`](../game_tests/20260825_clickwheel_input.md)
- Text/name-entry evidence: [`20260825_tetris_text_and_name_entry.md`](../game_tests/20260825_tetris_text_and_name_entry.md)
- Gameplay controls evidence: [`20260826_tetris_gameplay_controls.md`](../game_tests/20260826_tetris_gameplay_controls.md)
- Board-origin and long-run evidence: [`20260826_tetris_board_origin.md`](../game_tests/20260826_tetris_board_origin.md)
- Incremental-transform regression: [`20260826_tetris_incremental_transform_regression.md`](../game_tests/20260826_tetris_incremental_transform_regression.md)
- Frame capture produces PPM files visible with `open /tmp/tetris_capture_*.ppm`
