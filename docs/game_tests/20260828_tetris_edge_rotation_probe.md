# Tetris edge-rotation probe

Date: 2026-08-28  
Bundle: `66666` (`Tetris_1_1_2563292.bin`)

This is a narrow follow-up to the rotation receipt. It drives the active
piece to each outer playable column, rotates it at the boundary, and checks
the presented framebuffer for clipping or a fatal guest transition.

## Reproduce

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,wheel=4:300-310,left:320-321,right:350-351,wheel=-4:370-380,left:390-391,right:420-421,down:460-461' \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_tetris_edge_kick_20260828_b \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=500 \
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=500 \
FLIWHEEL_EAPP_STOP_FRAME=500 \
RUST_LOG='EAPP_INPUT=info,EAPP_GL=warn,EAPP_AUDIO=warn,EAPP_IMPORT=warn,EAPP=warn' \
target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/66666' \
  --headless --cycles 65000000
```

Receipt:

```text
/tmp/fliwheel_tetris_edge_kick_20260828_b.log
/tmp/fliwheel_tetris_edge_kick_20260828_b/manifest.tsv
```

## Result

- Exit status: `0`; 501 manifest rows (`0..500`), no fatal signature, panic,
  or process error.
- The centered well's outer playable red-piece bounds are `x=105..212` in
  this capture. The right sweep reaches `x=182..212`; `left` then rotates the
  piece to `x=193..212` without crossing the right edge, and `right` returns
  it to `x=182..212`.
- The reverse sweep reaches `x=105..135`; `left` rotates it to `x=116..135`
  without crossing the left edge, and `right` returns it to `x=105..135`.
- The `down` edge still produces the expected 16-to-47 draw transition for
  hard-drop/lock handling.

This verifies wall-adjacent rotation smoke for the observed piece and both
rotation directions. It does not yet prove a collision-dependent kick: the
tested rotations fit their edge positions without requiring an unambiguous
translation. Formal kick tables, blocked rotations against settled cells,
piece sequencing, scoring, persistence, and long-run parity remain open.
