# Bejeweled (Bundle 55555)

**Status:** 🟡 POPCAP BOARD/TUTORIAL PARTIAL | **Evidence:** the legacy filesystem contract now reaches the menu, 8×8 board, and built-in tutorial without a fatal; normalized wheel input reaches the guest, but target-gem selection is still unresolved | **Engine:** PopCap Engine

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
Complete the tutorial's target-gem selection and wheel-tap contract, then
verify adjacent-gem swaps, board updates, and the title's audio/save behavior.
The HLE now preserves the physical 96-detent position but emits the guest's
normalized 256-unit wheel ring. `0x1402000c` is part of the observed
pixel-write stream, so it should not be treated as a guessed completion
register without new evidence.

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
sweep delivers changing, normalized positions to the guest input object and
moves the guest cursor, but does not yet advance the tutorial. The linked input
event probe confirms that event 2 is the title's action/select edge and event 1
is menu/back; the target-gem/tap consumer still needs to be reached and
verified. See the [wheel normalization receipt](../game_tests/20260826_bejeweled_wheel_normalization.md).
This remains a verified board/tutorial boundary rather than a playable-game
claim.
