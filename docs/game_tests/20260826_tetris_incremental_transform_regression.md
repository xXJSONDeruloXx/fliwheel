# Tetris incremental-transform regression receipt

Date: 2026-08-26  
Bundle: `66666`  
Purpose: verify that centering the full Tetris board did not break the
low-draw incremental piece updates.

## Result

The regression is fixed. Full gameplay frames learn the matrix origin from
`matrix_565.pix` at `(102, 7)`. When the guest switches to its 12-draw
incremental path, the HLE now preserves the guest-composed base translation
from the first `0x13`/`0x19` material bind and carries it across the paired
matrix-cell and active-cell draws. This restores the active piece at the top
of the centered well without hard-coding its local position.

## A/B evidence

The same scripted replay was run against the repaired current build and the
known-good pre-centering revision `a19a465`.

- Current capture: `/tmp/fliwheel_tetris_pair_guestbase_20260826/`
- Reference capture: `/tmp/fliwheel_tetris_a19_ab_20260826/`
- Compared frame: `221`, the first stable 12-draw incremental frame
- Current and reference presented hash: `0xc1ba4c05066a165f`
- Current and reference PPM SHA-256:
  `efd3aa48c678478ca45548afc385f807f812738d5947318473a3c79adc5f84e5`
- Byte comparison: `cmp` returned `0`

The compared image contains the red active piece at the top-center of the
board. The receipt demonstrates renderer parity for this transition; it does
not establish complete Tetris parity.

## Verification

```bash
cargo test -p fliwheel-core --lib
cargo build --release -p fliwheel-desktop --bin eapp
git diff --check
```

The focused replay used the existing Tetris menu-entry script, captured every
frame through frame 225, and stopped cleanly. Frames 215 through 224 each
contain the expected paired six `0x13` and six `0x19` draws with the stable
presented hash above.

## Remaining Tetris gates

Wheel displacement during gameplay, exact rotate/move/drop mapping, line
clear behavior, save persistence, longer visual comparison, and physical
audio/mixer parity remain open.
