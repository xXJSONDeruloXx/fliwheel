# Royal Solitaire readiness-contract probe

Date: 2026-08-26 UTC  
Repository: `fliwheel`  
Bundle: `50514` (`Solitaire_1_1_2703086.bin`)

## Finding

Royal Solitaire is not waiting because the clickwheel events or savefile
request are missing. Its event list is populated and consumed, the savefile
request completes, and the recurring callback continues to run. The guest
state machine remains at state `1` because the manager byte at
`0x180cfa5c` stays `2` instead of reaching the title's ready value.

The state machine calls `0x180039f4` from `0x1802bd74` on every frame. The
helper reads that byte and only allows the state transition when it returns
zero. The normal fliwheel path never writes the byte. Static analysis finds a
clear operation in `0x1800ce0c`, reached through an object-method path, but no
execution of that path in the normal run.

## Reproduction

The bounded PC trace is opt-in and has no effect on normal execution:

```text
CLICKY_EAPP_PC_TRACE='0x1802bd68,0x1802bd74,0x1800389c,0x180037fc,0x180039f4,0x1800cdf0,0x1800ce0c,0x1800c960,0x18025394,0x1801f650,0x1801f6dc,0x1801f708,0x18005a24,0x180035c4,0x1800e39c,0x1800e418,0x180058ac' \
CLICKY_EAPP_PC_TRACE_LIMIT=12 \
INPUT_SCRIPT='action:18-20,action:45-47,action:80-82,wheel=3:130-132,left:180-182,right:230-232' \
RUST_LOG='EAPP_PC_TRACE=info,EAPP=info,EAPP_IMPORT=warn' \
./scripts/test_decrypted_games_interactive.sh \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO' 50514
```

The captured trace is `/tmp/fliwheel_royal_pc_trace_20260826_b/run.log`.
The input-focused progress run is
`/tmp/fliwheel_royal_action_progress_20260826_b/run.log`. It shows action
nodes entering the app event list and being consumed on the next input poll;
no board transition follows.

The savefile evidence is in
`/tmp/fliwheel_royal_async_baseline_20260826_b/run.log`: the request is
queued, its callback is dispatched, and the staged result is recorded. No
additional resource completion appears before the readiness stall.

The reference eApp loader's generic input stub is not a faithful control for
this title: it queues a raw `0x40000000 | code` word rather than the linked
event nodes Royal Solitaire consumes. Its init-vector run therefore confirms
the vector order but cannot establish the title's ready transition.

## Rejected workaround

A diagnostic-only run cleared `0x180cfa5c` at guest frame 5. The title then
entered states `5`/`6`, but the same helper cleaned up the manager object and
the captured frames remained the splash artwork. It did not reach a board or
menu. Evidence:

```text
/tmp/fliwheel_royal_force_ready_20260826_b/run.log
/tmp/fliwheel_royal_force_capture_20260826_b/
```

That result is useful for identifying the gate, but it is not a valid HLE
fix. The force-ready hook was removed; only the bounded read-only PC trace is
retained for future reverse engineering.

## Current status

The shared NDC projection fix makes the splash spatially coherent, and the
input lifecycle is now verified. Board initialization, card selection, save
state, and sound remain unverified. The next implementation target is the
missing manager/object readiness contract, not another input mapping or an
unconditional gate clear.
