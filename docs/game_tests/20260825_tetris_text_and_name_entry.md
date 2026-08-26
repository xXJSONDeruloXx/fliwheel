# Tetris text rendering and first-run name entry

Date: 2026-08-25

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
- The bounded input runs complete without a fatal memory error, panic, or
  unsupported-upload fatal.

Evidence artifacts:

- Readable credits frame: `/tmp/fliwheel_tetris_atlasfix_20260825/frame4_zoom.png`
- Name-entry frame: `/tmp/fliwheel_tetris_atlasfix_20260825/startup_g000014_host*.ppm`
- Input sequence log: `/tmp/fliwheel_tetris_name_sequence_20260825.log`
- Post-name frame: `/tmp/fliwheel_tetris_name_sequence_20260825/startup_g000050_host*.ppm`

## Reproduce

```bash
cargo build -p clicky-desktop --bin eapp

CLICKY_EXPERIMENTAL_GL_HLE=1 \
CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 \
CLICKY_GL_PRESENT_VFLIP=1 \
CLICKY_GL_DUMP_FRAMES=20 \
CLICKY_EAPP_ASYNC3_COMPLETE=1 \
CLICKY_EAPP_HOST_EVENT_FLAGS=0x10 \
CLICKY_EAPP_HOST_EVENT_DELAY=50 \
CLICKY_STARTUP_CAPTURE_DIR=/tmp/fliwheel_tetris_atlasfix_20260825 \
RUST_LOG='EAPP_GL=info,EAPP_PROGRESS=info,EAPP_IMPORT=warn,EAPP=warn' \
timeout 20s target/debug/eapp \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/66666'
```

The input sequence used for the character-commit evidence was:

```bash
CLICKY_EAPP_INPUT_SCRIPT='wheel=1:15-25,action:30-35'
```

## Current boundary

This is not yet a gameplay-ready Tetris result. After the first character is
committed, the guest transitions through its menu/exit state (`frame_state` 5
then 6) and stops issuing normal GL draw frames. The normal main-menu labels
are known to be constructed by the guest, but the active first-run scene root
still points at the name-entry graph. A valid profile/save transition or the
remaining scene-state contract is needed before Play can be selected and the
Tetris board can be tested.

The WAV resources are staged during boot, but the host audio sink and the
gameplay audio ABI are still unimplemented. No sound-correctness claim is
made by this checkpoint.

## Regression gates

```bash
cargo test -p clicky-core --lib eapp
cargo build -p clicky-desktop --bin eapp
git diff --check
```
