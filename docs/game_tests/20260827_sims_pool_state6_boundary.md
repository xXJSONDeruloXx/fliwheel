# Sims Pool state-6 transition boundary

Date: 2026-08-27  
Bundle: `1500E` (Sims Pool)  
Runner: `target/debug/eapp` with the experimental live GL HLE

## Finding

With the pointer-valued `gameLib.rlb` probe and the title's relocated
input-manager byte, Pool reaches its guest-owned post-load state transition.
The aux routine at `0x1804fa68` enters the state-5 branch at `0x1804fd14`,
writes frame state `6` at `0x1804fd50`, and then continues through the stable
state-6 path on later frames. No menu or gameplay scene draw follows this
transition.

The input-ready write remains diagnostic-only. It is title-specific: Pool's
matching input-manager global is `0x18085bac`, while Bowling uses
`0x1807380c`. Pool's separate `0x18086514` byte is checked later by the
state-5 branch and is not the trigger used by this probe.

## Post-state-6 input check

A bounded input script was delivered after state 6 (`action`, `menu`, and
wheel edges). The `InputEvents:0` import produced valid event-list heads, but
the guest's shared dispatcher at `0x18005000` found no active listener:
`[0x18085bac + 4] == 0`. The guest routine at `0x18006c04` intentionally
clears that listener at `0x18006c28` as part of the same state-6 teardown.

An opt-in retention probe restored the observed listener pointer
(`0x10006540`) immediately before the clear. That did not activate a scene;
the stale object instead faulted in its teardown path at `0x1801f3f0`
(`fault_addr=0x2fff0000`). The probe was removed. This rules out the current
input event-list ABI as the immediate blocker and points to missing guest-side
scene/object construction after state 6.

Receipts: `/tmp/fliwheel_sims_pool_post_state6_input_20260827.log`,
`/tmp/fliwheel_sims_pool_input_watch_20260827.log`, and
`/tmp/fliwheel_sims_pool_keep_listener_20260827.log`.

## Reproduction

The positive run used the Sims RLB completion gates, the pointer-valued owner
result, and the input-ready probe:

```text
FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE=1
FLIWHEEL_EAPP_SIMS_ASYNC0_OWNER_RESULT=payload
FLIWHEEL_EAPP_SIMS_ASYNC1_COMPLETE=1
FLIWHEEL_EAPP_SIMS_ASYNC2_COMPLETE=1
FLIWHEEL_EAPP_SIMS_INPUT_READY=1
FLIWHEEL_EAPP_SIMS_INPUT_READY_FRAME=60
FLIWHEEL_EAPP_PC_TRACE=0x1804fa68,0x1804fbc4,0x1804fc18,0x1804fd14,0x1804fd50,0x1804fda0
FLIWHEEL_EAPP_PC_TRACE_LIMIT=120
FLIWHEEL_EAPP_STOP_FRAME=80
```

The run logs the one-shot write as:

```text
Sims input readiness probe title=1500E addr=0x18085bac status=0x02 -> 0 wrote=true
```

At frame 61, `0x1804fd14` observes frame state `5` and
`0x1804fd50` executes the guest's state-6 write. At frame 62 and later,
`0x1804fd14` observes frame state `6`; the live GL path continues presenting
the existing partial title/progress frame without a new menu or gameplay
draw. Receipt: `/tmp/fliwheel_sims_pool_input_correct_20260827.log`.

## Interpretation

- Pool's full RLB is staged and guest-parsed through multiple real reads.
- The relocated readiness byte can reproduce the guest's stable state-6
  transition under an explicit probe.
- State 6 still does not activate a verified menu or gameplay object.

The next Pool target is scene/object activation after state 6, not another
shared input-status write.
