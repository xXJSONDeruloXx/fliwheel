# Pac-Man (Bundle AAAAA)

**Status:** ✅ WORKS | **Draws:** 24,511 (10s) | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/pacman.sh
./scripts/pacman.sh --timeout 15
./scripts/pacman.sh --headless
```

## Bundle Info
- **Executable:** `Pacman_1_1_2563976.bin` (eapp format)
- **Splash:** `PM_Logo.raw.lcd5` (RGB565)
- **Asset Format:** `.wav` (16 files) + `.tga` (2 files)

## Assets
- **Audio:** `audio/` directory with game sounds
- **Textures:** `tex_ig.tga`, `tex_menu1.tga` (TGA format)
- **Resources:** `Resources/` with localized content

## Notable
- Sister game to Ms. Pac-Man, similar engine
- Uses TGA texture format (rare among working games)
- Full attract mode rendering

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
