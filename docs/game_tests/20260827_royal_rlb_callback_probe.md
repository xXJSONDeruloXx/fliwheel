# Royal Solitaire RLB callback probe

Date: 2026-08-27 UTC  
Repository: `fliwheel`  
Bundle: `50514` (`Solitaire_1_1_2703086.bin`)

## Result

This probe did not produce an accepted default-path HLE change. Three
title-scoped, opt-in completion gates were used to separate the file payload
from the guest resource contract:

- `EAPP_ROYAL_ASYNC0_COMPLETE=1` completes the initial `q.wav` stream.
- `EAPP_ROYAL_ASYNC2_COMPLETE=1` completes the following staged read.
- `EAPP_ROYAL_ASYNC1_COMPLETE=1` completes the linked request with the
  observed `q.wav` byte count (`EAPP_ROYAL_ASYNC1_BYTES=452776`).

With all three enabled, the natural guest callback chain runs, Royal reaches
state 7, and the full `Solitaire.rlb` payload is staged and delivered. The
RLB opener then loops before the card board is created. The default runtime
remains on the no-forced-completion path.

The default runtime is unchanged and remains on the no-forced-completion
path.

## Evidence

The all-gate diagnostic established that the file payload itself is available
to the guest:

- `q.wav` was staged and read through its header/data path.
- `Solitaire.rlb` was staged at 13,180,905 bytes.
- The guest entered the RLB opener at `0x180112dc` after the callback chain
  completed.
- The RLB object's inner async-file object was constructed at
  `0x1002d434`, but its status at `object+0x10` stayed `-1`. The poll at
  `0x1800b898` therefore returned `-1` and `0x180112dc` repeated on later
  frames.

The manager trace shows the normal callback path rather than a missing generic
dispatcher continuation: `0x180238c4` falls through to `0x180238f8`, invokes
`0x1801eb90`, and that calls `0x1801ee40`. The latter updates the resource
manager and the guest proceeds to state 7. The remaining transition from the
resource-manager completion to the inner async-file object's ready status is
not implemented by the current HLE.

The focused run was bounded and produced no guest fault after the RLB stage:

```text
/tmp/fliwheel_royal_manager_watch_20260827.log
/tmp/fliwheel_royal_inner_watch_20260827.log
/tmp/fliwheel_royal_request_watch_20260827.log
```

## Current implication

Royal Solitaire remains at a coherent splash in the default path with
verified input/save lifecycle, while its board is not reached. The next
implementation target is the guest async-file object's readiness transition
after the resource-manager callback, not an unconditional ready-byte clear or
a direct callback queue. The completion gates are retained only as explicit
diagnostic probes and do not affect normal execution.
