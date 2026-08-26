# Texas Hold'em (Bundle 33333)

**Status:** 🟡 LOADING SCREEN ONLY | **Evidence:** corrected default-contract run completes 30,000,000 cycles / 700 captured frames with no fatal, but stabilizes on `LOADING` | **Engine:** Hold'em Runtime

## Quick Start
```bash
./scripts/holdem.sh
./scripts/holdem.sh --timeout 15
./scripts/holdem.sh --headless
```

## Bundle Info
- **Executable:** `HoldEm_1_1_2563291.bin` (eapp format)
- **Splash:** `Holdem.raw.lcd5` (RGB565)
- **Asset Format:** `.ipd` (111 files) + `.blob` (15 files)

## Assets
- **Textures:** `.ipd` files loaded via `AsyncFileIO:3`
- **Background music:** `c.m4a`, `t.m4a`
- **Characters:** `Characters/` directory
- **Locations:** `Locations/` directory
- **Fonts:** `Fonts/Euro/ArialBold15.ipd` (A8 alpha font atlas)
- **Resources:** `Data/textures.txt`, localization in `Resources/`

## Notable
- Uses `Filesytem` import module (but doesn't depend on it for init)
- Loads `.ipd` font atlases successfully through AsyncFileIO:3
- Second-highest draw count — full poker table rendering
- The current default path is safe but not yet playable: the common scripted
  input schedule leaves the title on its loading screen. The experimental
  `CLICKY_EAPP_ASYNC3_COMPLETE=1` completion fields are Tetris-only and must
  not be used as Hold'em evidence; they exercise a different resource ABI.

## Scoped experimental progress (2026-08-26)

The default matrix remains the authority and still reports loading-only. A
title-scoped experimental path now completes Hold'em's resource callbacks,
decodes its `GL_PALETTE8_RGBA8_OES` indexed textures, and confirms the
first-run name screen. A deterministic wheel/action sweep reaches the first
post-name game scene: the capture reaches 113 draws at guest frame 553 and
the longer rerun reaches a 107-draw scene at guest frame 607. That scene is
still incomplete (missing/garbled UI and later blank transitions), so this is
resource/rendering progress, not a playable-game claim.

Reproduction requires all of the following title-scoped overrides in addition
to the normal GL HLE environment:

```bash
EAPP_TEXAS_ASYNC0_COMPLETE=1 EAPP_TEXAS_ASYNC0_STATUS=1 \
EAPP_TEXAS_ASYNC2_COMPLETE=1 EAPP_TEXAS_ASYNC1_COMPLETE=1
```

Evidence:

- Name-to-scene sweep: `/tmp/fliwheel_holdem_ok_sweep_20260826/`
- Detailed post-name run: `/tmp/fliwheel_holdem_table_20260826/`
- The indexed texture decoder and callback path are opt-in and title-scoped;
  they do not alter the corrected corpus-wide default run.

## Current evidence
- Corrected matrix: `/tmp/fliwheel_holdem_matrix_20260826/interactive_matrix.md`
- 30,000,000-cycle log: `/tmp/fliwheel_holdem_matrix_20260826/logs/33333.log`
- 100 startup captures: `/tmp/fliwheel_holdem_matrix_20260826/captures/33333/`

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
