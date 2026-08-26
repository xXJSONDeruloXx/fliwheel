# Sims Pool (Bundle 1500E)

**Status:** ❌ RENDERER/ASSET DECODE BLOCKED | **Evidence:** normalized-geometry rerun remains black with zero useful coverage | **Engine:** Sims Engine

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

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
