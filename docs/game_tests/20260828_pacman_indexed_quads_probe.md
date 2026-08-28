# PAC-MAN indexed GL_QUADS probe

Date: 2026-08-28 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Result

PAC-MAN's ordinal-38 call is an indexed `GL_QUADS` batch, not the indexed
triangle-strip path used by the Sims/Sudoku/Solitaire family. The guest passes
`mode=7`, `type=GL_UNSIGNED_SHORT`, a fixed index pointer `0x10112e60`, and
`count=960` at the start of the maze. The count decreases in multiples of four
as pellets are consumed, which is consistent with one four-index quad per
pellet.

The live HLE now decodes the index stream, enabled fixed-point position/UV
arrays, and rasterizes each four-index group through the existing textured-quad
path. In the comparable bounded route:

- the previous default log recorded 275 unsupported `mode=7` calls;
- the new log recorded 274 rasterized indexed-quad batches and zero unsupported
  `mode=7` calls;
- batches ranged from 240 quads (`count=960`) down to 223 quads
  (`count=892`);
- the route reached lifecycle frame 1048 with zero draw skips and no fatal,
  panic, fault, or segmentation signature.

The frame-800 capture visibly restores the complete pellet field. Pac-Man,
ghosts, HUD, `READY`, lives, score, and the maze geometry remain coherent. The
same scripted quadrant route still moves Pac-Man and changes the score from
`0` to `30` and then `40`.

This is a renderer/content milestone. It does not certify repeated lives,
full-stage completion, exit/save, persistence, physical audio mixing, or
long-run parity.

## Reproduction

```bash
cargo build --release -p fliwheel-desktop --bin eapp

env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_AUDIO_TRACE=1 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_TRACE=1 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_INTERVAL=25 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:100-252,action:260-265,action:300-305,action:350-355,action:750-755,bits=0x400000f0:800-830,bits=0x40000030:850-880,bits=0x40000070:900-930,bits=0x400000b0:950-980' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_pacman_indexed_quads_capture_20260828 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=10 \
  FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=780 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=1050 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=40 \
  FLIWHEEL_EAPP_STOP_FRAME=1050 \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA' \
  --headless --cycles 650000000 \
  > /tmp/fliwheel_pacman_indexed_quads_20260828.log 2>&1
```

The new capture manifest is
`/tmp/fliwheel_pacman_indexed_quads_capture_20260828/manifest.tsv`. The
frame-800 image is
`startup_g000800_host000048398043_hash299e06fed5e9f11a.ppm` in that directory.
The pre-change frame-800 comparison is retained in
`/tmp/fliwheel_pacman_movement_20260828/`.

## Regression receipts

- `cargo test -p fliwheel-core --lib live_gl::tests`: 26 passed.
- `cargo test -p fliwheel-core --lib eapp`: 40 passed.
- Tetris control: 499 lifecycle rows through frame 498, including frame 400,
  zero actual skipped-draw lines, zero unsupported indexed calls, and zero
  fatal/panic/fault signatures in
  `/tmp/fliwheel_tetris_indexed_quads_info_20260828.log`.
- Sims Bowling indexed-triangle-strip control: 5,386 lifecycle rows through
  frame 5,385, 5,385 rasterized indexed strips, zero skips, and zero fatal
  signatures in
  `/tmp/fliwheel_simsbowling_indexed_quads_regression_20260828.log`.
- Ms. PAC-MAN control: no ordinal-38 `mode=7` calls in the route; its prior
  maze UV-edge result remains unchanged. The only residual there is the known
  no-UV handle-`0x2` draw.

