# Clickwheel input contract

Date: 2026-08-25

This note records the first shared clickwheel contract recovered from the
decrypted titles. It is an input-ingress result, not a claim that the games
are already fully playable.

## Guest packet

The decrypted Tetris and Sudoku consumers independently decode `InputEvents:0`
as an absolute wheel position:

```text
bit 30       = wheel report present
bits 0..7    = absolute position on a 96-detent circle
bits 8..29   = unused by the observed consumers
```

The HLE now keeps a persistent `0..95` position and converts host scroll
callbacks into that packet. A host delta is consumed once at the next guest
poll, so a single scroll event is not replayed on every frame. Button input
continues to use the existing edge-linked event list and held-state mask.

Headless scripts support `wheelup`, `wheeldown`, and explicit relative deltas:

```sh
FLIWHEEL_EAPP_INPUT_SCRIPT='wheelup:700-710,action:740-745'
FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=-2:300-305'
```

Unit coverage verifies the absolute position and wrap behavior:

```text
cargo test -p fliwheel-core wheel_packet -- --nocapture
2 passed
```

## Runtime evidence

The focused Tetris run used the parsed-resource path and captured frames:

```text
/tmp/fliwheel_next_audit/tetris_wheel_action.log
/tmp/fliwheel_next_audit/tetris_wheel_action/frame700.png
/tmp/fliwheel_next_audit/tetris_wheel_action/frame740.png
```

The input trace contains the expected sequence:

```text
frame 700  bits=0x40000002
frame 701  bits=0x40000004
...
frame 710  bits=0x40000016
frame 740  bits=0x00000010  action edge
```

The packet reaches the guest without a fatal exception. The frame-700 and
frame-740 captures remain the same splash image, so this pass proves ingress
but does not yet prove that Tetris's first-run/name-entry scene transitions.

The same packet shape was observed while exercising Sudoku, Solitaire, PAC-MAN,
and Ms. PAC-MAN. Sudoku's static consumer masks bit `0x40000000`, extracts the
low byte, computes a signed delta from its previous position, and dispatches
the wheel event through its own state machine.

## Current interpretation

- The host-to-guest wheel ABI is now measured and shared across the family.
- Tetris still selects its first-run/name-entry scene; ordinary wheel and
  action edges do not yet select the constructed main-menu graph.
- Audio is now a separate, partially instrumented path. Tetris resource-indexed
  events resolve to `Menu.wav`, `Move.wav`, and `Drop.wav`, and the headed
  frontend has a `rodio` sink. Physical output, overlap/mixing, and the Audio
  ABI for the other title families remain unverified; see
  [`20260825_audio_abi.md`](20260825_audio_abi.md).

For external visual/input references, see the [clickwheel games overview](https://ipodwiki.com/wiki/Clickwheel_games),
the [Tetris-on-iPod reference photograph](https://commons.wikimedia.org/wiki/File:Tetris_on_an_iPod.jpg),
and the [preservation project's title/version matrix](https://github.com/Olsro/ipodclickwheelgamespreservationproject).
