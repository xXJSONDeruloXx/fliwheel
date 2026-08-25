# Bejeweled (Bundle 55555)

**Status:** ⚠️ DMA WAIT | **Draws:** 180 (then hangs) | **Engine:** PopCap Engine

## Quick Start
```bash
# Partially works - renders loading screen then hangs on DMA
./target/release/eapp /Users/kurt/Downloads/16-ipod-games/Games_RO/55555
```

## Issue
Game renders 180 draws (loading screen text) then writes to hardware DMA
register at `0x1402_000c`. The HW stub prevents a crash but the game
enters a spin-wait for DMA completion that never arrives.

Game prints "Abnormal termination" and exits when the DMA doesn't respond.

## Bundle Info
- **Executable:** `Bejeweled_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.pix` + `.tga` (1 file)

## Fix Needed
DMA controller emulation: respond to writes at `0x1402_000c` by setting
a completion status bit that the game reads back.

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
