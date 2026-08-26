# Tetris board-origin and long-run renderer checkpoint

Date: 2026-08-26  
Bundle: `66666`  
Purpose: validate the board transform after the full gameplay frame switches
from the low-draw update pattern to the 47+ draw composition.

## Result

The Tetris matrix is now centered in full gameplay frames. The guest's four
ordinal-169 components immediately before the `matrix_565.pix` draw sum to
`(102, 7)`, which is the expected origin for the 115×223 rasterized matrix on
the 320×240 display. The HLE now captures that origin from the matrix draw
itself, rather than treating the earlier full-screen background material as
the board base, and restores it for the `0x19` cell material.

The first active cell after the matrix is consequently rasterized at
`(146, 18)-(157, 29)` with non-zero coverage instead of at the left edge or
off-screen.

This fixes a real frame-boundary renderer defect. It does not establish full
Tetris parity: later piece/preview and line-clear paths still contain
off-screen transforms that need separate guest-array/ABI analysis, and host
audio output remains unverified.

## Evidence

Focused 50M-cycle run:

- Log: `/tmp/fliwheel_tetris_board_fix2_20260826.log`
- Manifest/captures: `/tmp/fliwheel_tetris_board_fix2_20260826/`
- Frame-400 capture: `/tmp/fliwheel_tetris_board_fix2_20260826/startup_g000400_host000015931646_hash4718a20aa4566bfd.ppm`
- Exit: `0`
- Captured rows: `430`
- Unique presented hashes: `72`
- Hash changes: `72`
- Maximum draws: `382`
- Zero-draw rows: `2`

Representative GL records from frame 400:

```text
draw37 handle=0x13 dim=(115,223) bounds=(102.0,7.0)-(217.0,230.0) cov=25645
draw39 handle=0x19 dim=(11,11) bounds=(146.0,18.0)-(157.0,29.0) cov=121
```

Long repeat-drop/game-over regression:

- Log: `/tmp/fliwheel_tetris_repeat_drop_20260826_c.log`
- Manifest/captures: `/tmp/fliwheel_tetris_repeat_drop_20260826_c/`
- Exit: `0`
- Captured rows: `1000`
- Unique presented hashes: `109`
- Hash changes: `110`
- Maximum draws: `382`
- Zero-draw rows: `2`
- Guest resource events: `Drop.wav` at frames 400, 470, 540, 610, 680,
  750, 820, 890, and 960; `Lock.wav` at frames 1411, 1793, and 2028;
  `GameOver.wav` at frame 2028.

The long run used `EAPP_AUDIO_DISABLE=1`, so those are resource-identity
events, not proof of host speaker output or mixer parity.

## Reproduce

```bash
cargo build --release -p clicky-desktop --bin eapp

CLICKY_EXPERIMENTAL_GL_HLE=1 \
CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 \
CLICKY_GL_PRESENT_VFLIP=1 \
CLICKY_EAPP_ASYNC3_COMPLETE=1 \
CLICKY_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,left:300-301,right:330-331,wheel=1:360-360,down:400-401,down:470-471,down:540-541,down:610-611,down:680-681,down:750-751,down:820-821,down:890-891,down:960-961' \
EAPP_AUDIO_DISABLE=1 \
EAPP_AUDIO_EVENT_TRACE=1 \
target/release/eapp \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/66666' \
  --headless --cycles 100000000
```
