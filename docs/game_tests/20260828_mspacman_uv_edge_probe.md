# Ms. PAC-MAN maze UV-edge probe

Date: 2026-08-28 UTC  
Bundle: `14004`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004`

## Result

The maze texture upload is `256x256` (`tex_maze_blue.bin`, GL texture name
`0x0f`), while the guest's maze triangle strip reports UVs ending at
`v=257`. The old strict containment test computed a required height of 257
and dropped the draw, producing 434 triangle-strip upload skips in the prior
default log.

The live texture matcher now accepts only the observed one-pixel integer-edge
overrun, preserving strict containment for larger or fractional overruns. The
focused route then ran through guest frame 749 with zero triangle-strip upload
skips and no fatal, panic, or fault signature. The frame-620 capture shows the
complete Stage 1 maze, HUD, Pac-Man, ghosts, pellets, and life icons.

This fixes a renderer association boundary; it does not certify gameplay,
audio mixer behavior, persistence, or full-content parity.

## Reproduction

```bash
cargo build --release -p fliwheel-desktop --bin eapp

env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='action:100-105,wheel=1:125-130,action:145-150,wheel=1:165-170,action:185-190,wheel=-1:205-222,action:230-235,wheel=-1:245-262,action:280-285,action:340-345,action:380-385,action:430-435,action:600-605,bits=0x400000f0:650-680,bits=0x40000030:700-730,bits=0x40000070:750-780,bits=0x400000b0:800-830' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_mspacman_uv_edge_gameplay_20260828 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=50 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=750 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=20 \
  FLIWHEEL_EAPP_STOP_FRAME=750 \
  RUST_LOG='EAPP_GL=info,EAPP=warn,EAPP_IMPORT=warn,EAPP_AUDIO=warn,EAPP_INPUT=warn' \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004' \
  --headless --cycles 320000000 \
  > /tmp/fliwheel_mspacman_uv_edge_gameplay_20260828.log 2>&1
```

For a visual-only maze capture, the same route was run with
`FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=600` and produced:

- Capture directory: `/tmp/fliwheel_mspacman_uv_edge_maze_capture_20260828/`
- Manifest: `/tmp/fliwheel_mspacman_uv_edge_maze_capture_20260828/manifest.tsv`
- Stage 1 capture: `startup_g000620_host000012867305_hashe0ab74111e9cbae0.ppm`
- Log: `/tmp/fliwheel_mspacman_uv_edge_maze_capture_20260828.log`

## Regression receipts

- Core `eapp` library tests: 38 passed.
- Tetris controlled route: 499 completed guest frames, 27,443 rasterized
  draw records, zero actual skipped-draw lines, and zero fatal signatures in
  `/tmp/fliwheel_tetris_uv_edge_regression_info_20260828.log`.
- Ms. PAC-MAN focused route: 749 completed guest frames, zero
  `no live upload matched triangle-strip UV span` lines, and zero fatal,
  panic, or fault signatures in
  `/tmp/fliwheel_mspacman_uv_edge_gameplay_20260828.log`.
