# Bejeweled (Bundle 55555)

**Status:** 🟡 CLASSIC + ACTION BOARD ENTRY VERIFIED / WAV EVENTS PARTIAL | **Evidence:** the legacy filesystem contract reaches the menu, 8×8 board, and built-in tutorial; normalized wheel input reaches the guest; Classic mode has a scripted live-board match that changes/refills tiles and emits `combo2.wav`; Action mode reaches a live board with its timer bar; the title-specific WAV ABI maps all 27 sources and reaches the headed desktop sink | **Engine:** PopCap Engine

## Quick Start
```bash
# Set GAMES_RO to the directory containing the decrypted bundles.
GAMES_RO=/path/to/Games_RO
./target/release/eapp "$GAMES_RO/55555"
```

## Issue
The current HLE reaches the resource-backed GL path and exits cleanly. The
legacy `Filesytem` open/read/close ABI loads `tweakdata.txt`, reaches the
selectable menu, builds the 8×8 gem board, and presents the built-in tutorial.
The corrected wheel scale and sector mapping now drive a complete scripted
Classic-mode match: the guest selects the board cell at `[8,6]`, swaps it with
`[8,7]`, shows “EXCELLENT!”, changes the board contents, and opens the
score-bar overlay. A separate headed menu probe selects Action mode and reaches
the live board with its timer bar. Multi-move Action play, save behavior, and
physical mixer parity remain open.

## Bundle Info
- **Executable:** `Bejeweled_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.pix` + `.tga` (1 file)

## Fix Needed
Verify the translated wheel/tap gesture in a headed run, then repeat the match
in both modes and cover the title's audio/save behavior. The
HLE preserves the physical 96-detent position but emits the guest's
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

Focused evidence: [PopCap DMA contract probe](../game_tests/20260826_popcap_dma_contract.md),
[Bejeweled input event contract](../game_tests/20260826_bejeweled_input_event_contract.md),
and the [Action-mode probe](../game_tests/20260828_bejeweled_action_mode_probe.md).

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
moves the guest cursor. Event 2 is the title's action/select edge and event 1
is menu/back. See the [wheel normalization receipt](../game_tests/20260826_bejeweled_wheel_normalization.md).

The original live-match receipt is:

```text
/tmp/fliwheel_bejeweled_match_candidate_right_20260826_capture/
/tmp/fliwheel_bejeweled_match_candidate_right_20260826.log
```

The input trace reaches `0x1801667c` and `0x18017e68` at guest frame 1172,
then reports `nav+0x4` returning from swap state 4 to normal state 2. The
capture shows “EXCELLENT!” during refill and a score-bar overlay afterward;
selected visual frames are in:

```text
/tmp/fliwheel_bejeweled_match_candidate_right_20260826_png/frame_1250.png
/tmp/fliwheel_bejeweled_match_candidate_right_20260826_png/frame_1325.png
```

This is a verified playable Classic core path under the deterministic input
script, and the desktop runner now exposes the same directional tap mapping
through the arrow keys. A fresh current-tree replay also reproduces an
accepted swap and `combo2.wav`; see the [current match replay](../game_tests/20260827_bejeweled_current_match_replay.md).
The Action menu entry and timer-bar board are now separately verified; an
accepted Action-mode match, save path, and broader audio coverage are still
open.

The title-specific audio receipt is in the [Bejeweled WAV ABI report](../game_tests/20260827_bejeweled_audio_abi.md).
The earlier exact replay maps 27/27 startup WAV sources and emits
`audio/swap.wav` followed by `audio/bad.wav` for the observed accepted and
rejected swap attempts. The newer current-tree replay reaches match
resolution and emits `audio/combo2.wav`; combo3/excellent audio and physical
mixer parity remain unverified. The headed run produced matching `played
sound` receipts, so host decode/sink dispatch is verified.
