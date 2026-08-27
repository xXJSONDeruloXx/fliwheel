# Sims Bowling state-6 transition boundary

Date: 2026-08-27  
Bundle: `1500C` (Sims Bowling)  
Runner: `target/debug/eapp` with the experimental live GL HLE

## Finding

With the pointer-valued `gameLib.rlb` probe enabled, Bowling parses real RLB
entries and reaches a title-specific state transition late in startup. The
guest aux routine at `0x180455e4` sees app state `5`, writes frame state `6` at
`0x180458cc`, and on the next frame takes the stable state-6 return path. No
new menu or gameplay draw is scheduled after that transition.

The input-ready write is necessary for this transition in the current HLE
run, but it is not the scene handoff itself. The no-input control reaches only
a transient frame state `5` and returns to state `1`. The probe therefore stays
diagnostic-only; there is no evidence for making it a default host behavior.

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
FLIWHEEL_EAPP_PC_TRACE=0x180455e4,0x18045740,0x18045794,0x18045890,0x180458cc,0x1804591c
FLIWHEEL_EAPP_PC_TRACE_LIMIT=100
FLIWHEEL_EAPP_PC_TRACE_DETAIL=1
FLIWHEEL_EAPP_STOP_FRAME=75
```

At frame 61 the trace enters `0x18045890` with app state `5`, then reaches
`0x180458cc`. At frame 62 and later it re-enters `0x18045890` with app state
`6` and returns through `0x1804591c`; the framebuffer hash is unchanged and
the live GL path reports no new scene draw.

The focused write watch covered `0x18073800..0x180739ff` during frames 60--62.
It recorded the state-object setup and completion fields. The guest-PC trace
additionally captured the frame-context write at `0x10502b00`:

| Frame | Address | Value | Writer PC | Meaning |
| ---: | ---: | ---: | ---: | --- |
| 60 | `0x18073854` | `1` | `0x18007704` | state object reinitialized |
| 60 | `0x1807385b` | `1` | `0x18045888` | aux state flag |
| 61 | `0x1807385d` | `1` | `0x180458b4` | async/save completion flag |
| 61 | `0x10502b00` | `6` | `0x180458cc` | frame context enters state 6 |
| 61 | `0x18073860` | timestamp | `0x1804594c` | state-object time update |

The exact watch receipt is `/tmp/fliwheel_sims_bowling_state_watch_20260827.log`.
The guest-PC receipt is `/tmp/fliwheel_sims_bowling_aux_trace_20260827.log`.

## Control

The same Sims async gates without `FLIWHEEL_EAPP_SIMS_INPUT_READY` produced
the same RLB parsing and title rendering, but remained at frame state `1`
after a transient state-5 observation. It did not reach stable state `6` by
frame 66. Receipt: `/tmp/fliwheel_sims_bowling_no_input_probe_20260827.log`.

## Interpretation

This narrows Bowling's current blocker beyond the RLB stream itself:

- the full RLB is staged and guest-parsed;
- multiple guest-derived resource reads complete;
- the guest can reach its stable post-load state 6;
- the state-6 path does not activate a menu/gameplay object or issue a new
  scene draw in the current HLE.

The next target is the scene/object activation or render dispatch expected
after state 6. The input-ready write remains an opt-in reverse-engineering
instrument and is not a correctness fix.
