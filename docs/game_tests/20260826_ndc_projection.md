# Normalized-coordinate projection regression

Date: 2026-08-26

Bundles: `50513` Sudoku, `50514` Royal Solitaire, `1500C` Sims Bowling,
`1500E` Sims Pool

## Finding

The first NDC renderer workaround normalized each draw's own min/max bounds to
the full 320x240 framebuffer. That made every small sprite fill the screen.
Royal Solitaire exposed the defect clearly: its 43x56 and 24x40 character/UI
textures were rendered as full-screen slabs instead of at their submitted
coordinates.

The HLE now uses the shared normalized projection observed in the NDC family:

```text
pixel_x = normalized_x / 1.2 * 320
pixel_y = normalized_y / 0.9 * 240
```

The transform is global to the frame, so a sprite at normalized bounds
`(1.1, 0.7)`–`(1.2, 0.9)` occupies approximately pixels `(293, 187)`–
`(320, 240)` rather than the entire surface.

## Evidence

The focused probe used the shared input schedule and a 20,000,000-cycle
budget:

```bash
CYCLES=20000000 \
RUN_ROOT=/tmp/fliwheel_ndc_projection_regression_20260826 \
INPUT_SCRIPT='menu:450-452,action:550-552,wheel=3:650-652,left:750-752,right:850-852,up:950-952,down:1050-1052' \
./scripts/test_decrypted_games_interactive.sh \
  /path/to/Games_RO 1500C 1500E 66666
```

The Royal Solitaire visual probe and Sudoku comparison are in
`/tmp/fliwheel_ndc_projection_probe_20260826/`.

```text
50513 Sudoku: exit 0, 1 hash, 1 change, max 1 draw, no fatal signatures
50514 Royal Solitaire: exit 0, 26 hashes, 59 changes, max 3 draws, 638 zero-draw frames, no fatal signatures
1500C Sims Bowling: exit 0, 2 hashes, 3 changes, max 2 draws, no fatal signatures
1500E Sims Pool: exit 0, 2 hashes, 3 changes, max 2 draws, no fatal signatures
66666 Tetris: exit 0, 16 hashes, 16 changes, max 382 draws, no fatal signatures
```

Royal Solitaire's first steady scene now consists of a full background plus
small, correctly placed character pieces. This is a renderer milestone, not a
playability claim: the board/state transition and card selection controls are
still unverified.
