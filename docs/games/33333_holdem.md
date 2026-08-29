# Texas Hold'em (Bundle 33333)

**Status:** 🟡 DEFAULT TABLE ROUTE; HAND/CONTENT OPEN | **Evidence:** the normal runtime now reaches a coherent green poker table at guest frame 902 in a 30,000,000-cycle run with no fatal signature; hand flow, sound traversal, and persistence remain open | **Engine:** Hold'em Runtime

## Quick Start
```bash
./scripts/games/holdem.sh
./scripts/games/holdem.sh --timeout 15
./scripts/games/holdem.sh --headless
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
- The default path now completes the measured resource lifecycle and reaches a
  coherent table with player panels, blinds, pot, and the local player name.
  Gameplay remains open.
- The three Hold'em async stages are enabled by default for bundle `33333`.
  Set `EAPP_TEXAS_ASYNC0_COMPLETE=0`, `EAPP_TEXAS_ASYNC1_COMPLETE=0`, or
  `EAPP_TEXAS_ASYNC2_COMPLETE=0` to disable an individual stage for an A/B
  diagnostic. `FLIWHEEL_EAPP_ASYNC3_COMPLETE=1` remains Tetris-only and is not
  part of the Hold'em contract.
- The `Sounds/sounds.blob` length table is now validated and extracted into
  27 temporary WAV sources. Startup `Audio:0` registrations map bank indices
  `0..26` to those sources. The tested hand route did not yet produce an
  `Audio:2` playback commit, so audible gameplay and mixer timing remain open.

## Historical scoped progress (2026-08-26)

The first title-scoped experiment completed Hold'em's resource callbacks,
decoded its `GL_PALETTE8_RGBA8_OES` indexed textures, and confirmed the
first-run name screen. It reached the first post-name scene at 113 draws and
was retained as the evidence that justified the later default promotion.
Those old captures are historical; the current default receipt is the
2026-08-29 report below.

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

## Direct-oracle parity (2026-08-29)

The scoped path now covers two previously missing pieces of the direct PR #3
render contract:

- Hold'em's first `OpenGLES:45` call is `glGenTextures(1, out)` even though
  its unused third register is `0xf0`. The measured callsite is restricted to
  the Hold'em initializer (`LR=0x180073a4`), so the ordinary generated name is
  `1` without weakening the other ordinal-45 resource-descriptor paths.
- Hold'em uses `OpenGLES:169/173/175` to build a translated/rotated card
  matrix, then uploads it through `OpenGLES:125`. The matrix helper and vertex
  projection are now enabled only for Hold'em and the already-supported Vortex
  path.

Evidence from a fresh corrected run:

- Fliwheel rendered the 47-quad card/menu transition at guest frames 490-496
  without a fatal: `/tmp/fliwheel_33333_holdem_matrix_20260829d/`.
- The aligned Fliwheel frame 496 and direct-oracle frame 508 images differ by
  at most 2 in any RGB channel, with 97.31% of channel bytes identical and a
  mean absolute channel error of 0.0327. The direct image is
  `/tmp/ipod-shot-06.png`; the Fliwheel image is
  `/tmp/fliwheel_33333_holdem_matrix_20260829d/startup_g000496_host000065324207_hash2ace7ee63440c307.ppm`.
- The canonical control script retains later checkpoints through frame 2000:
  [`pr3_holdem_name_controls.script`](../../scripts/oracle/pr3_holdem_name_controls.script).

This is strong menu/card and default-table rendering parity, not a
perfect-playability claim. Poker actions, save behavior, and full audio
traversal still need their own evidence.

## Current evidence
- Default table route: [2026-08-29 default async receipt](../game_tests/20260829_holdem_default_async.md)
- Sound-bank source route: [2026-08-29 Hold'em audio bank receipt](../game_tests/20260829_holdem_audio_bank.md)
- Corrected matrix: `/tmp/fliwheel_holdem_matrix_20260826/interactive_matrix.md`
- 30,000,000-cycle log: `/tmp/fliwheel_holdem_matrix_20260826/logs/33333.log`
- 100 startup captures: `/tmp/fliwheel_holdem_matrix_20260826/captures/33333/`

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
