# Bejeweled (Bundle 55555)

**Status:** 🟡 POPCAP LOADING/CONTENT PARTIAL | **Evidence:** 30M-cycle interactive probe reaches the PopCap resource/GL path and presents the loading spinner, but not a coherent board | **Engine:** PopCap Engine

## Quick Start
```bash
# Set GAMES_RO to the directory containing the decrypted bundles.
GAMES_RO=/path/to/Games_RO
./target/release/eapp "$GAMES_RO/55555"
```

## Issue
The current HLE reaches the resource-backed GL path and exits cleanly. A
30,000,000-cycle interactive probe presents the centered loading spinner with
the corrected PopCap texture-bind association, but it does not reach a
coherent board or an actionable game menu. The older DMA-only completion
diagnosis is no longer supported by the current bounded run.

## Bundle Info
- **Executable:** `Bejeweled_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.pix` + `.tga` (1 file)

## Fix Needed
Continue the resource-backed path past the spinner: identify the loading-state
completion/readiness contract, then map the board texture/material sequence.
`0x1402000c` is part of the observed pixel-write stream, so it should not be
treated as a guessed completion register without new evidence.

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
# PopCap titles default to the guest screen origin; set this explicitly only
# for orientation A/B experiments.
```

Focused evidence: [PopCap DMA contract probe](../game_tests/20260826_popcap_dma_contract.md).
