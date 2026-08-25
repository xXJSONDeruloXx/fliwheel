# Mahjong (Bundle 77777)

**Status:** ✅ WORKS | **Draws:** 21,235 (10s) | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/mahjong.sh
./scripts/mahjong.sh --timeout 15
./scripts/mahjong.sh --headless
```

## Bundle Info
- **Executable:** `Mahjong_1_1_2563294.bin` (eapp format)
- **Splash:** `mahjong.raw.lcd5` (RGB565)
- **Asset Format:** `.m4a` (10 files) + `.rlb` resource bundle

## Assets
- **Audio:** Multiple `.m4a` music tracks (22.m4a through 69.m4a)
- **Resources:** `main.rlb` (resource library bundle)

## Notable
- Uses `.rlb` resource library bundle format
- No `.pix` or `.ipd` files — textures likely embedded in .rlb

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
