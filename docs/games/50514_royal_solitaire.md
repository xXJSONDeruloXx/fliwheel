# Royal Solitaire (50514)

Current state: 🟡 coherent splash/input verified; default resource startup
stalls, while an opt-in probe reaches the RLB opener; board not reached.

The title renders a spatially coherent splash after the shared normalized
coordinate projection fix. Scripted clickwheel action, wheel, and directional
events reach and drain the guest event list, and the savefile request
completes. In the clean default path, the state machine reaches state 5 and
waits at the initial `q.wav` completion. A title-scoped diagnostic completion
probe advances through state 7 and stages the full `Solitaire.rlb`; the guest
then constructs its inner async-file object but leaves its status at `-1` and
repeats the RLB opener. The card board is not reached. Forcing the readiness
byte clear only tears down the manager and leaves the splash, so the default
runtime is unchanged.

See the detailed [readiness-contract probe](../game_tests/20260826_royal_readiness.md)
and [RLB callback probe](../game_tests/20260827_royal_rlb_callback_probe.md)
and the [current interactive matrix](../game_tests/20260826_interactive_matrix.md#current-matrix).

Next: reconstruct the guest async-file-object readiness transition after the
resource-manager callback, then verify the card board, selection/drag
behavior, persistence, and sound.
