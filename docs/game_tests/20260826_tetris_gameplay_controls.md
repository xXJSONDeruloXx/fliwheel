# Tetris gameplay controls and timing

Date: 2026-08-26

This is the follow-up gameplay checkpoint for bundle `66666`, after the
first-run name-entry, menu, controls, and board-entry work documented in
[`20260825_tetris_text_and_name_entry.md`](20260825_tetris_text_and_name_entry.md).

The subsequent centered-board and long-run renderer checkpoint is documented
separately in [`20260826_tetris_board_origin.md`](20260826_tetris_board_origin.md).

## Verified

- The initial board scene is reachable through the normal scripted first-run
  path.
- A center-button press after board entry pauses the scene; a second center
  press resumes it. With the live guest clock advancing, the active piece then
  visibly falls through the 10×20 well.
- A left-button edge changes the active piece draw/shape, and a right-button
  edge produces a subsequent draw/shape change. This verifies guest delivery
  and a responsive side-button gameplay path, though exact physical labels are
  still being compared with device references.
- A down-button edge triggers the hard-drop/lock transition: the active piece
  moves to the floor, the board performs a high-draw transition, and the next
  piece scene appears.
- A sustained signed wheel sweep reaches the guest as an absolute-position
  packet and visibly moves the active piece horizontally. The isolated
  before/after receipt is [`20260826_tetris_wheel_probe.md`](20260826_tetris_wheel_probe.md).
  Exact physical scaling and device labels remain open.
- The bounded run completes without a fatal memory error, panic, unsupported
  upload fatal, or emulator process error.

## Evidence

- Manifest: `/tmp/fliwheel_tetris_play_controls_probe_20260826/manifest.tsv`
- Log: `/tmp/fliwheel_tetris_play_controls_probe_20260826.log`
- Captures: `/tmp/fliwheel_tetris_play_controls_probe_20260826/`
- Pause/resume captures: `/tmp/fliwheel_tetris_pause_toggle_probe_20260826/`
- Controls page reference capture: `/tmp/fliwheel_tetris_auto_controls_probe_20260826_png_early/g209.png`

The play-controls manifest records these useful boundaries:

| Guest frame | Input/effect | Observable result |
|---:|---|---|
| 230 | center press | pause transition begins |
| 260 | center press | gameplay resumes |
| 300 | left press | draw/hash changes |
| 330 | right press | draw/hash changes |
| 360 | wheel packet `0x4000002a` | packet delivered; displacement still open |
| 400 | down press | hard-drop/lock transition begins |

The later centered-board isolation uses `wheel=4:240-245` followed by
`wheel=-4:270-275`. The dominant red active-piece component moves from
`x=149..168` to `x=193..212`, then back to `x=127..157`; see the dedicated
receipt for the exact capture paths and metrics. The hard-drop transition is
now centered by the title-scoped board re-anchor; lock, line-clear, and
next-piece correctness remain separate gameplay gates.

The external [iLounge Tetris review](https://www.ilounge.com/index.php/reviews/entry/electronic-arts-tetris)
describes the same clickwheel-oriented control family: wheel sweeps for
movement, side buttons for rotation, and down/center actions for dropping.

## Reproduce

```bash
cargo build --release -p fliwheel-desktop --bin eapp

FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,left:300-301,right:330-331,wheel=1:360-360,down:400-401' \
FLIWHEEL_STARTUP_PROGRESS_TRACE=1 \
FLIWHEEL_STARTUP_PROGRESS_INTERVAL=50 \
FLIWHEEL_STARTUP_PROGRESS_FRAMES=200 \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_tetris_play_controls_probe_20260826 \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=500 \
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=500 \
RUST_LOG='EAPP_PROGRESS=info,EAPP_INPUT=info,EAPP_GL=warn,EAPP=warn,EAPP_IMPORT=warn' \
target/release/eapp \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/66666' \
  --headless --cycles 50000000
```

## Current boundary

Tetris now has evidence-backed gameplay timing, pause/resume, gravity, wheel
movement, side-button response, and a centered hard-drop transition. It is not
yet a full parity result: exact rotate/move/drop labels, lock and line-clear
behavior, save persistence, long-run rendering, and host audio playback remain
open. Device comparison is still needed for those gameplay semantics and
transition details.
