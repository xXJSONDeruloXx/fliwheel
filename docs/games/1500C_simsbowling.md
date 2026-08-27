# Sims Bowling (Bundle 1500C)

**Status:** 🟡 TITLE SCREEN + PARTIAL FOLLOW-UP | **Evidence:** shared NDC projection now rasterizes the title screen; gameplay/menu transition is not yet reached | **Engine:** Sims Engine

## Quick Start
```bash
./scripts/games/simsbowling.sh
./scripts/games/simsbowling.sh --timeout 15
./scripts/games/simsbowling.sh --headless
```

## Bundle Info
- **Executable:** `SimsBowling_1_1_3002478.bin` (eapp format)
- **Asset Format:** `.wav` (31 files) + `.rlb` resource bundle

## Assets
- **Audio:** `.wav` and `.m4a` files (a-g musical notes + sfx)
- **Resources:** `gameLib.rlb` (game library bundle)

## Notable
- Sims engine variant with different asset loading
- Uses `.rlb` resource library format
- Lower draw count — simpler UI than other games
- The title screen is now spatially coherent after fixing the lowercase runtime
  bundle ID (`1500c`) in the shared NDC projection matcher. Bowling then enters
  a one-draw follow-up state that is currently only partially rendered.

See the [2026-08-27 NDC casing-fix probe](../game_tests/20260827_sims_ndc_casing_fix.md)
for the exact command, coverage, hashes, and captures.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
