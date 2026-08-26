# Tetris text rendering and first-run name entry

Date: 2026-08-26

This is the current focused checkpoint for bundle `66666`. It supersedes the
older exploratory notes in [`tetris-text-rendering.md`](../tetris-text-rendering.md)
for present status; that file remains as historical RE material.

## Verified

- Parsed boot reaches the real first-run name-entry scene with the live GL HLE.
- Credits and UI text are readable English rather than collapsed glyphs or a
  Japanese menu atlas.
- The renderer selects `f8x10text1_a8.pix` for the measured 8×10 English glyph
  stream, even though the guest reuses texture handle `0x8` for many A8 assets.
- Generated glyph draws accumulate the guest's per-glyph translation deltas
  when the font remains bound on an ordinary GL texture handle.
- The wheel moves the highlighted character (`A` → `B` with positive wheel
  input), and Select commits the highlighted character into the player-name
  field.
- The scripted first-run path reaches `MENU`, `PICK GAME`, the `CONTROLS`
  tutorial, and the initial gameplay board.
- The live renderer preserves Tetris' framebuffer after the guest's explicit
  color clear and carries the paired tile translations during low-draw
  incremental updates. The full board and well remain visible while the
  falling-piece and projected-piece draws stay inside the well.
- Left, right, wheel, and action events each change the observed draw/hash
  stream during the board probe.
- The bounded input runs complete without a fatal memory error, panic, or
  unsupported-upload fatal.

Evidence artifacts:

- Readable credits frame: `/tmp/fliwheel_tetris_atlasfix_20260825/frame4_zoom.png`
- Name-entry frame: `/tmp/fliwheel_tetris_atlasfix_20260825/startup_g000014_host*.ppm`
- Input sequence log: `/tmp/fliwheel_tetris_name_sequence_20260825.log`
- Post-name frame: `/tmp/fliwheel_tetris_name_sequence_20260825/startup_g000050_host*.ppm`
- Board-entry manifest: `/tmp/fliwheel_tetris_auto_controls_probe_20260826/manifest.tsv`
- Board-entry captures: `/tmp/fliwheel_tetris_auto_controls_probe_20260826/`
- Board-entry log: `/tmp/fliwheel_tetris_auto_controls_probe_20260826.log`
- Full corpus report: `/tmp/fliwheel_regression_20260826_auto/20260825_221028_decrypted_games.md`

## Reproduce

```bash
cargo build -p clicky-desktop --bin eapp

CLICKY_EXPERIMENTAL_GL_HLE=1 \
CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 \
CLICKY_GL_PRESENT_VFLIP=1 \
CLICKY_EAPP_ASYNC3_COMPLETE=1 \
CLICKY_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,left:260-261,right:280-281,wheel=1:300-300,action:320-321' \
CLICKY_STARTUP_CAPTURE_DIR=/tmp/fliwheel_tetris_auto_controls_probe_20260826 \
CLICKY_STARTUP_CAPTURE_PERIOD=1 \
CLICKY_STARTUP_CAPTURE_MAX_FRAMES=350 \
CLICKY_STARTUP_CAPTURE_MAX_DUMPS=350 \
RUST_LOG='EAPP_GL=info,EAPP_PROGRESS=info,EAPP_IMPORT=warn,EAPP=warn' \
target/release/eapp \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/66666' \
  --headless --cycles 35000000
```

The input sequence used for the full first-run and board-entry evidence was:

```bash
CLICKY_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,left:260-261,right:280-281,wheel=1:300-300,action:320-321'
```

## Current boundary

This is not yet a fully playable Tetris result. The scripted path now reaches
the board, and the renderer keeps the 10×20 well intact while dynamic tile
draws change in response to input. The remaining behavioral gates are gravity,
lock/line-clear, rotation/drop mapping, save persistence, and long-run visual
comparison against a real-device reference. Public references describe the
same 10×20 well and clickwheel-oriented control scheme; see the
[iLounge review](https://www.ilounge.com/index.php/reviews/entry/electronic-arts-tetris)
and the [archived iPod gameplay photo](https://commons.wikimedia.org/wiki/File:Tetris_on_an_iPod.jpg).

The older no-input run used `CLICKY_EAPP_HOST_EVENT_FLAGS=0x10`, which was a
diagnostic host-event injection and is not a valid no-input control. A clean
no-host-event run remains in `frame_state` 1 through its sampled startup
window. The old state-5/state-6 conclusion is retained only in the historical
RE notes, not as the current board-entry status.

The selector query at `0x18018cb8` was also probed with guest results `0`, `2`,
and `8`. All three variants still construct the name-entry scene and follow
the same state-5/state-6 path, so this query is a keyboard/profile-layout
selector rather than the missing main-menu switch.

The WAV resources are staged during boot, but the host audio sink and the
gameplay audio ABI are still unimplemented. No sound-correctness claim is
made by this checkpoint.

## Regression gates

```bash
cargo test -p clicky-core --lib eapp
cargo build -p clicky-desktop --bin eapp
git diff --check
```
