# Vortex PR #3 Select transition

Date: 2026-08-28 UTC  
fliwheel commit: input-bridge checkpoint after `9772646`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/12345`  
Oracle: `/tmp/ipod-emulator-pr3/target/release/play` at PR #3

## Scope

This is a direct decrypted-eApp comparison. Neither run boots an IPSW. The
oracle's Vortex defaults use its discovered button-flags word at
`0x18063e5c`, where Select is bit `0x01`; fliwheel now mirrors its logical
Select and Menu states into that same title-local word while retaining the
generic `InputEvents:0` packet.

The reproducible oracle script is
[`scripts/oracle/pr3_vortex_select.script`](../../scripts/oracle/pr3_vortex_select.script).

## Evidence

Oracle command:

```text
/tmp/ipod-emulator-pr3/target/release/play \
  .../12345/Executables/vortex_1_1_2563290.bin \
  --script=scripts/oracle/pr3_vortex_select.script --fixed-clock --fps=0 \
  --gamedir=.../Games_RO/12345
```

At frame 500 the oracle reports `select -> flags bit 0x01`. Its frame-650
screenshot is the `ENTER NAME` screen, with the rocky background, circular
selector, alphabet ring, counters, and the green selected `A`.

fliwheel command:

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_FIXED_CLOCK=1 \
FLIWHEEL_VORTEX_PR3_GL165=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:500-505' \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_vortex_select_fix_20260828/captures \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=490 \
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=180 \
target/release/eapp .../Games_RO/12345 --headless --cycles 65000000
```

The run is clean (`exit=0`, no fatal signature). The capture changes from the
colored title at frame 499 to the transition at frame 501 and a settled
`ENTER NAME` screen by frame 540. The post-transition manifest remains at 46
draws per frame through frame 1199, confirming that this is a real scene change
and not only a framebuffer-side effect.

The input log records the scripted action at frames 500-505. The new bridge is
title-gated to bundle `12345`; it does not write this address for other games.

## Assessment

This closes Vortex's first input transition: title art and Select now agree in
kind with the PR #3 direct runner. Name-entry navigation, character selection,
confirmation, gameplay controls, audio, pause/return, and long-run content
parity remain open.
