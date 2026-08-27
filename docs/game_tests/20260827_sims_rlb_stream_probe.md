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

Useful receipts from the focused Bowling run:

- `/tmp/fliwheel_sims_async123_63527_20260827.log`
- `/tmp/fliwheel_sims_async123_capture_20260827/manifest.tsv`
