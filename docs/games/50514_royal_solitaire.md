# Royal Solitaire (50514)

Current state: 🟡 coherent splash and input lifecycle verified; board not
reached.

The title renders a spatially coherent splash after the shared normalized
coordinate projection fix. Scripted clickwheel action, wheel, and directional
events reach and drain the guest event list, and the savefile request
completes. The guest remains in its readiness state because the manager
contract at `0x180cfa5c` is never completed; forcing that byte clear only
tears down the manager and leaves the splash.

See the detailed [readiness-contract probe](../game_tests/20260826_royal_readiness.md)
and the [current interactive matrix](../game_tests/20260826_interactive_matrix.md#current-matrix).

Next: reconstruct the manager/object readiness callback, then verify the card
board, selection/drag behavior, persistence, and sound.
