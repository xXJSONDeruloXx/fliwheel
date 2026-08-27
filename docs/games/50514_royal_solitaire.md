# Royal Solitaire (50514)

Current state: 🟡 coherent splash/input verified; default resource startup
stalls, while an opt-in probe reaches the RLB opener; board not reached.

The title renders a spatially coherent splash after the shared normalized
coordinate projection fix. Scripted clickwheel action, wheel, and directional
events reach and drain the guest event list, and the savefile request
completes. In the clean default path, the state machine reaches state 5 and
waits at the initial `q.wav` completion. A title-scoped diagnostic completion
probe advances through state 7 and stages the full `Solitaire.rlb`. Without the
owner-payload probe the guest copies `-1` into its resource payload field and
repeats the RLB opener; with `EAPP_ROYAL_ASYNC0_OWNER_RESULT=payload`, it
consumes the staged pointer and issues the first 4,096-byte RLB read. The
longer request cycle still does not reach the card board. Forcing the
readiness byte clear only tears down the manager and leaves the splash, so the
default runtime is unchanged.

See the detailed [readiness-contract probe](../game_tests/20260826_royal_readiness.md)
and [RLB callback probe](../game_tests/20260827_royal_rlb_callback_probe.md)
and the [current interactive matrix](../game_tests/20260826_interactive_matrix.md#current-matrix).

Next: reconstruct the post-RLB stream/request lifecycle after the
owner-payload handoff, then verify the card board, selection/drag behavior,
persistence, and sound.
