# Royal Solitaire RLB callback probe

Date: 2026-08-27 UTC  
Repository: `fliwheel`  
Bundle: `50514` (`Solitaire_1_1_2703086.bin`)

## Result

This probe did not produce an accepted HLE change. It tested whether the
ordinal-1 completion could safely queue Royal Solitaire's linked request
callback (`0x1801ee18`) after the transient owner callback (`0x18023930`).
The direct callback path is not sufficient: it either leaves the resource
load stalled or faults in guest code before the next resource can load.

The default runtime is unchanged and remains on the no-forced-completion
path.

## Evidence

The earlier all-gate diagnostic established that the file payload itself is
available to the guest:

- `q.wav` was staged and read through its header/data path.
- `Solitaire.rlb` was staged at 13,180,905 bytes.
- The RLB parser read 4,096 bytes, then read 98,216 bytes from the staged
  offset 4,096 into the guest resource buffer.
- The first resource-table entry still remained at state `0`/`1` while the
  two earlier title resources reached state `6`; the state-7 readiness check
  therefore returned false.

The focused linked-callback attempts were run with temporary, title-scoped
completion gates and then removed:

- Directly queuing `0x1801ee18` with the observed request context did not
  advance beyond the initial `q.wav` chain; the run completed with the splash
  still shown (`/tmp/fliwheel_royal_linked_20260827/`).
- Queuing the internal owner shim followed by the linked callback caused a
  guest read fault at `0x18022f80` from address `0x00000008` at frame 16
  (`/tmp/fliwheel_royal_linked_b_20260827/`).

The fault shows that the missing piece is the firmware dispatcher’s
continuation and context/owner lifecycle, not a simple callback address or a
missing RLB payload. `PendingGuestCall` currently does not propagate callback
return registers, so it cannot yet model the owner shim's transformed
`(request,status,count)` result safely.

## Current implication

Royal Solitaire remains at a coherent splash with verified input/save
lifecycle, while its board is not reached. The next implementation target is
an explicit model of the ordinal-1 dispatcher continuation and the resource
manager's completion state transition. No unconditional ready-byte or forced
callback workaround is accepted.
