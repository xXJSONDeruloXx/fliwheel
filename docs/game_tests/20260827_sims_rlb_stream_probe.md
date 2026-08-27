# Sims RLB resource-stream probe

Date: 2026-08-27
Bundles: `1500C` (Sims Bowling), `1500E` (Sims Pool)
Runner: `target/release/eapp` with the experimental live GL HLE

## Finding

The Sims preload callback does not hand the game a byte count as its resource
result. It hands the resource manager a pointer to the staged `gameLib.rlb`
image. With that pointer-valued result, the guest begins parsing the RLB and
issues the real seek/read sequence itself.

This is a diagnostic path only. The normal HLE path remains unchanged, and
neither title reaches a verified menu or gameplay scene yet.

## Probe flags

```text
FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE=1
FLIWHEEL_EAPP_SIMS_ASYNC0_OWNER_RESULT=payload
FLIWHEEL_EAPP_SIMS_ASYNC2_COMPLETE=1
FLIWHEEL_EAPP_SIMS_ASYNC1_COMPLETE=1
```

`payload` is a named alias for the current synthetic guest address of the
staged whole-file image. `FLIWHEEL_EAPP_SIMS_ASYNC1_STATUS` and
`FLIWHEEL_EAPP_SIMS_ASYNC1_BYTES` are retained as probe controls; the observed
resource callback primarily consumes the callback transition, so the byte
count is not yet treated as a parsed RLB field.

## Observed stream

The first operation is common to both titles:

1. preload the complete `gameLib.rlb`;
2. seek to offset `0`;
3. read the first 4096 bytes into the guest parser buffer;
4. complete the secondary async callback;
5. reopen the RLB, seek to the title's header marker, and read the next
   resource payload.

The title-specific first payloads were:

| Bundle | Header marker | First payload | Later observed payloads |
| --- | ---: | ---: | --- |
| `1500C` Bowling | `0x4a2` / 1186 | 63,527 bytes at RLB offset 4096 | 460,259 bytes at 7,978,152; 941 bytes at 1,567,654; 194,089 bytes at 3,491,970 |
| `1500E` Pool | `0x476` / 1142 | 55,607 bytes at RLB offset 4096 | 441,409 bytes at 7,679,180; 940 bytes at 1,301,170; 213,397 bytes at 3,244,580 |

Those offsets and lengths are guest-derived requests, not host-side guesses.
The HLE copies the requested bytes from the staged RLB into the guest
destination and records the stream offset for the next operation.

## Result

Both games autonomously parse several RLB entries and begin requesting sound
and other resource data. Bowling also reaches an `f.wav` request. The live
renderer then presents a mostly black frame with a progress-bar remnant; the
menu and gameplay scene are still absent. The next target is the resource
object/material handoff that follows these successful reads, plus the exact
secondary async status/byte contract.

## Input-readiness gate probe

The two Sims images use relocated input-manager globals while they are in
their resource-loading state. A separate Sims-only diagnostic can write `0`
after a chosen frame:

```text
FLIWHEEL_EAPP_SIMS_INPUT_READY=1
FLIWHEEL_EAPP_SIMS_INPUT_READY_FRAME=60
```

In the controlled Bowling run, the write was observed at frame 60 and the
guest advanced its frame state from `1` through `5` to `6`, but it produced no
new scene draws, menu, or gameplay. Injecting the existing input script at the
same time did not change that result. This rules out the status byte as a
sufficient content-handoff fix; it remains an opt-in diagnostic only. A
follow-up guest-PC trace shows that the final state change is guest-owned: the
aux routine sees app state `5`, writes frame state `6` at `0x180458cc`, and
then takes a stable state-6 return path. Without the input-ready probe, the
same RLB gates reach only a transient state `5` and return to state `1`.
The state-6 boundary and receipts are documented in the [Bowling state-6
probe](20260827_sims_bowling_state6_boundary.md).

Pool's matching write targets `0x18085bac`, found through the same relocated
input-manager helper that references Bowling's `0x1807380c`. With that
title-specific probe enabled, Pool enters its state-5 branch at
`0x1804fd14`, writes stable frame state `6` at `0x1804fd50`, and remains on
the state-6 path through the bounded run. The existing live frame does not
gain a menu or gameplay draw. Pool's `0x18086514` byte is a separate check in
the state-5 branch, not the readiness trigger. See the [Pool state-6
probe](20260827_sims_pool_state6_boundary.md) and receipt
`/tmp/fliwheel_sims_pool_input_correct_20260827.log`.

## Additional negative probes

The following experiments were kept title-scoped and opt-in:

- `FLIWHEEL_EAPP_SIMS_ASYNC1_STATUS=0` versus `1`, with either zero or
  63,527 callback bytes, produced the same loading boundary. The callback
  transition is therefore still modeled as the useful part of this stage;
  the exact status/byte meaning remains unresolved.
- `FLIWHEEL_EAPP_SIMS_ZERO_FILL_RSERVER=1` padded the short `rserver.bin`
  read from 105,020 bytes to its requested 512,000-byte buffer. Bowling still
  stayed in the same loading/progress path, so this is not sufficient either.
- With `FLIWHEEL_EAPP_PC_TRACE_DETAIL=1`, the guest resource object moved
  through its expected open/read/complete/close states and the high-level
  stream created successive objects. No single pending read or stuck callback
  explains the eventual stop; the next target is the parser's
  resource/material construction after those completed reads.

Useful receipts from the focused Bowling run:

- `/tmp/fliwheel_sims_async123_63527_20260827.log`
- `/tmp/fliwheel_sims_async123_capture_20260827/manifest.tsv`
