# Sudoku event lifecycle and RLB probe

Date: 2026-08-26

Bundle: `50513` (`Sudoku_1_1_2703081.bin`)

## Finding

Sudoku uses the reversed `InputEvents:0` register convention found in the
Solitaire-family wrapper: the work-RAM owner is in `r5`, while `r4` is empty.
The wrapper can also drop that owner on the poll immediately after creating an
event list. The HLE now remembers the reversed owner, defers clearing its
consumed head until the next poll, and relinks a later edge through that owner
when the wrapper passes zero.

This fixes the shared event-list lifetime without changing the normal Tetris
owner path. The focused regression completed cleanly for Sims Bowling, Sims
Pool, Royal Solitaire, and Tetris:

```text
1500C: exit 0, 454 frames, 2 hashes, 3 changes, max 2 draws, fatal 0
1500E: exit 0, 452 frames, 2 hashes, 3 changes, max 2 draws, fatal 0
50514: exit 0, 700 frames, 26 hashes, 57 changes, max 3 draws, fatal 0
66666: exit 0, 431 frames, 16 hashes, 16 changes, max 382 draws, fatal 0
```

The input trace for a press/release pair now shows the owner head being
created, consumed, and drained instead of remaining stale:

```text
frame 450: press   -> owner 0x10500b00, list 0x10502b80
frame 453: release -> owner 0x10500b00, list 0x10502b90
frame 550: press   -> owner 0x10500b00, list 0x10502ba0
frame 553: release -> owner 0x10500b00, list 0x10502bf0
```

## Sudoku state-machine boundary

Static disassembly shows that event ID 1 is the Menu/exit transition. It
clears the title runtime object and leaves the guest alternating its save and
settings states while waiting for that object, which explains the stable
splash followed by zero-draw frames. It is not the puzzle-start control.

The following sequence reaches that path without a fatal or stale event head:

```bash
CLICKY_EAPP_INPUT_SCRIPT='menu:450-452,action:550-552' \
CLICKY_EXPERIMENTAL_GL_HLE=1 CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 CLICKY_GL_PRESENT_VFLIP=1 \
./target/release/eapp /path/to/Games_RO/50513 --headless
```

The current default run remains at the title scene. The action path, raw
hardware-packet mapping, and puzzle board are not verified.

## Whole-RLB diagnostic probe

Sudoku's `Sudoku.rlb` is 16,409,519 bytes. The title-scoped opt-in below
stages the complete file at the guest owner payload and executes the observed
`AsyncFileIO:0` callback chain:

```bash
EAPP_SUDOKU_ASYNC0_COMPLETE=1 \
CLICKY_EAPP_INPUT_SCRIPT='action:450-452' \
CLICKY_EXPERIMENTAL_GL_HLE=1 CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 CLICKY_GL_PRESENT_VFLIP=1 \
./target/release/eapp /path/to/Games_RO/50513 --headless
```

The callback completes, but no additional GL imports or board draws follow;
the framebuffer remains the splash/title image. This is retained as negative
evidence and is not enabled by the default interactive matrix. The next
Sudoku investigation must identify the post-RLB scene contract and the actual
puzzle-start event rather than treating Menu as Select.
