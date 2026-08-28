# Ms. Pac-Man (Bundle 14004)

**Status:** ⚠️ CLEAN LOADING SCREEN ONLY | **Evidence:** title-scoped live-GL probe holds a stable Namco loading screen after all observed resource/audio callbacks; gameplay is not reached | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/games/mspacman.sh
./scripts/games/mspacman.sh --timeout 15
./scripts/games/mspacman.sh --headless
```

## Bundle Info
- **Executable:** `mspacman_1_1_2805293.bin` (eapp format)
- **Asset Format:** `.wav` (20 files), `MsPAC-MAN.raw.lcd5`, and 22 packed `.bin` texture atlases

## Assets
- **Audio:** 20 `.wav` files for game sounds (coin, die, eat ghost, fruit bounce, etc.)
- **Textures:** the `.bin` atlases are requested through the guest async-resource path and include fonts, UI, maze, fruit, tutorial, and gameplay sheets

## Notable
- Classic arcade game with simple but recognizable graphics
- The live renderer now preserves the untagged 512×256 launch upload for the
  unbound `0x19` material instead of selecting later font/UI atlases by size
- The guest still remains on the loading screen after the diagnostic completion
  probes; these overrides are not part of the default contract

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
