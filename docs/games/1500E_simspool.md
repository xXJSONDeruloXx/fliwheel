# Sims Pool (Bundle 1500E)

**Status:** 🟡 TITLE SCREEN RENDERED | **Evidence:** shared NDC projection now rasterizes a coherent title screen; gameplay/menu transition is not yet reached | **Engine:** Sims Engine

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
- The title screen is now spatially coherent after fixing the lowercase runtime
  bundle ID (`1500e`) in the shared NDC projection matcher. The follow-up
  one-draw state remains incomplete.

See the [2026-08-27 NDC casing-fix probe](../game_tests/20260827_sims_ndc_casing_fix.md)
for the exact command, coverage, hashes, and captures.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
