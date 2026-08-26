# Bejeweled (Bundle 55555)

**Status:** 🟡 POPCAP BOARD/TUTORIAL PARTIAL | **Evidence:** the legacy filesystem contract now reaches the menu, 8×8 board, and built-in tutorial without a fatal; wheel selection is still unresolved | **Engine:** PopCap Engine

## Quick Start
```bash
# Set GAMES_RO to the directory containing the decrypted bundles.
GAMES_RO=/path/to/Games_RO
./target/release/eapp "$GAMES_RO/55555"
```

## Issue
The current HLE reaches the resource-backed GL path and exits cleanly. The
legacy `Filesytem` open/read/close ABI now loads `tweakdata.txt`, reaches the
selectable menu, builds the 8×8 gem board, and presents the built-in “Selecting
Gems” tutorial. The remaining blocker is interactive cursor movement and
selection, not the earlier loading-spinner diagnosis.

## Bundle Info
- **Executable:** `Bejeweled_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.pix` + `.tga` (1 file)

## Fix Needed
Complete the tutorial's wheel-selection contract, then verify adjacent-gem
swaps, board updates, and the title's audio/save behavior. The wheel packet is
already delivered through the guest's bit-30/low-byte input object; the
remaining consumer/state transition needs a guest-side trace. `0x1402000c` is
part of the observed pixel-write stream, so it should not be treated as a
guessed completion register without new evidence.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
# PopCap titles default to the guest screen origin; set this explicitly only
# for orientation A/B experiments.
```

Focused evidence: [PopCap DMA contract probe](../game_tests/20260826_popcap_dma_contract.md)
and [Bejeweled input event contract](../game_tests/20260826_bejeweled_input_event_contract.md).

## Current interactive boundary

The focused board run used the first menu-select pulse only:

```text
/tmp/fliwheel_bejeweled_board_20260826/
```

It reached 700 guest frames, 98 unique presented hashes, 187 hash changes,
and a 179-draw peak with no fatal signature. A later tutorial run selected the
first tutorial page and reached the stable “Selecting Gems” step:

```text
/tmp/fliwheel_bejeweled_tutorial_20260826/
```

That run reached 1,200 guest frames and 175 unique hashes. The emulator's
`Filesytem:0` opens `tweakdata.txt`, `Filesytem:2` returns its sequential bytes,
and `Filesytem:1` closes the synthetic handle; the receipt is in
`/tmp/fliwheel_bejeweled_fs2_20260826/logs/55555.log`. A controlled wheel
sweep delivers changing positions to the guest input object but does not yet
advance the tutorial. The linked input event probe now confirms that event 2
is the title's action/select edge and event 1 is menu/back; the cursor/gameplay
consumer still needs to be reached and verified. This remains a verified
board/tutorial boundary rather than a playable-game claim.
