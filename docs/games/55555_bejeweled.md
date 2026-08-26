# Bejeweled (Bundle 55555)

**Status:** ⚠️ POPCAP DMA/CONTENT PARTIAL | **Evidence:** 50M-cycle probe completes a full DMA-buffer write but presents a malformed/warped scene | **Engine:** PopCap Engine

## Quick Start
```bash
# Partially works - renders GL loading content and a malformed DMA scene
./target/release/eapp /Users/kurt/Downloads/16-ipod-games/Games_RO/55555
```

## Issue
The current HLE reaches 37 GL draws and then writes all 153,600 bytes of the
320x240 DMA buffer. The final DMA-only capture has 76,794 non-zero pixels and
recognizable Bejeweled fragments, but the scene is malformed/warped and does
not reach a coherent board. The old “DMA completion never arrives” diagnosis
is no longer supported by the current bounded run.

## Bundle Info
- **Executable:** `Bejeweled_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.pix` + `.tga` (1 file)

## Fix Needed
Reverse the control/data contract around the `0x14000000` aperture and the
guest software-rendered buffer, then repair PopCap texture/board composition.
`0x1402000c` is part of the observed pixel-write stream, so it should not be
treated as a guessed completion register without new evidence.

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```

Focused evidence: [PopCap DMA contract probe](../game_tests/20260826_popcap_dma_contract.md).
