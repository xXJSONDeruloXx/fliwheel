# Sims Bowling (Bundle 1500C)

**Status:** ✅ WORKS | **Draws:** 3,296 (10s) | **Engine:** Sims Engine

## Quick Start
```bash
./scripts/simsbowling.sh
./scripts/simsbowling.sh --timeout 15
./scripts/simsbowling.sh --headless
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

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
