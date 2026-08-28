# Pac-Man (Bundle AAAAA)

**Status:** ⚠️ PARTIAL LOADING/BOARD SCENE | **Evidence:** fresh 700-frame probe reaches a stable board-like Namco scene with up to 57 draws, but no confirmed maze, menu, or playable input | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/games/pacman.sh
./scripts/games/pacman.sh --timeout 15
./scripts/games/pacman.sh --headless
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
- Partial board-like scene rendering; maze/menu/gameplay are not confirmed

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
