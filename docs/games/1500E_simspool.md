# Sims Pool (Bundle 1500E)

**Status:** 🟡 TITLE SCREEN + PARTIAL FOLLOW-UP | **Evidence:** shared NDC projection rasterizes the title and the ordinary atlas path now shows a small `The` follow-up text element; gameplay/menu transition is not yet reached | **Engine:** Sims Engine

## Quick Start
```bash
./scripts/games/simspool.sh
./scripts/games/simspool.sh --timeout 15
./scripts/games/simspool.sh --headless
```

## Bundle Info
- **Executable:** `SimsPool_1_1_3023310.bin` (eapp format)
- **Asset Format:** `.wav` (30 files) + `.rlb` resource bundle

## Assets
- **Audio:** `.wav` and `.m4a` files (similar to Sims Bowling)
- **Resources:** `gameLib.rlb` (game library bundle)

## Notable
- Sister game to Sims Bowling, shares gameLib.rlb
- Similar draw count and asset structure
- Sims engine variant
- The title screen is spatially coherent after fixing the lowercase runtime
  bundle ID (`1500e`) in the shared NDC projection matcher. The follow-up
  one-draw state now shows a small colored `The` text element through Pool's
  ordinary `297x75` atlas upload, but remains incomplete.

See the [2026-08-27 NDC casing-fix probe](../game_tests/20260827_sims_ndc_casing_fix.md)
and [rectangle-target texture probe](../game_tests/20260827_sims_paletted_texture_target.md)
for the exact commands, coverage, hashes, and captures.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```

The optional `FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE=1` flag is a diagnostic RLB
completion experiment only; it is not required for the current default
follow-up draw and does not yet reach gameplay.
