# EAPP contract iteration

Date: 2026-08-25

This note records the non-default runtime contracts added while bringing the
decrypted corpus forward. All experimental paths below are opt-in through
environment variables; the ordinary HLE path remains the regression target.

## Guest callback arguments

Pending guest callbacks now retain four arguments instead of only `r0`/`r1`.
The dispatcher restores `r0` through `r3` before entering the callback. This
is required for resource-loader callbacks whose request and status values are
not limited to the first two registers.

## Staged resource reads

The generic AsyncFileIO experiment can stage a whole host file, expose its
guest address through the request owner, and service subsequent reads while
preserving the staged-file offset. It is enabled only with:

```sh
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_ASYNC0_RESULT=length \
FLIWHEEL_EAPP_ASYNC2_GENERIC=1 \
FLIWHEEL_EAPP_ASYNC1_GENERIC=1 \
FLIWHEEL_EAPP_ASYNC1_STATUS=1
```

On Sudoku’s `Sudoku.rlb`, the first staged read consumed 4096 bytes at offset
0 and the next read consumed 153884 bytes at offset 4096. Preserving the
offset avoided the earlier repeated-offset null-table fault. The run then
settled cleanly with no pending async work, but it did not reach a third read
or prove full gameplay. This remains a loader investigation, not a default
compatibility claim.

## Hardware trace result

`FLIWHEEL_EAPP_HW_TRACE=1` and `FLIWHEEL_EAPP_HW_TRACE_READS=<budget>` now provide
bounded read/write traces for the hardware aperture. A Bejeweled trace showed
that writes near `0x1402000c` are framebuffer pixel offsets used by the DMA
overlay path, not a completion/status register. No separate DMA completion
poll was observed in the bounded trace. This negative result prevents treating
that pixel address as a guessed status contract.
