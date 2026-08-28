# Pac-Man (Bundle AAAAA)

**Status:** 🟡 PARTIAL MENU/STARTUP | **Evidence:** default resource loading completes; guest input reaches the main menu and START GAME screen, but initialization faults before the maze | **Engine:** Tetris Runtime

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
- **Textures:** `tex_ig.tga`, `tex_menu1.tga` (TGA format); the executable also references a missing `tex_menu.tga`
- **Resources:** `Resources/` with localized content

## Notable
- Sister game to Ms. Pac-Man, similar engine
- Uses TGA texture format (rare among working games)
- Positive wheel motion commits characters in name entry; the guest's real
  confirm path reaches the main menu and START GAME screen

## Current evidence

The normal path completes 20 asynchronous resource requests, renders the
Namco/name-entry scene, and receives both wheel and Select edges. A bounded
positive-wheel route commits `A`, `AB`, `ABG`, and `ABGL`. Holding the wheel
until the guest's confirm selector (`0x2c`) and pressing Select then reaches the
guest-rendered informational prompt, the main menu with `PLAY GAME` selected,
and the `START GAME / MODE / STAGE / BACK` screen.

Selecting `START GAME` advances the guest through initialization states 2
through 9, then faults before the first maze frame at `PC 0x1801628c` while
reading `0x58` through a null nested pointer. The experimental live-GL path
faults at frame 785; the legacy path reaches the same guest fault at frame 791
in the saved rerun, so this is not isolated to the live renderer. Maze
rendering, D-pad movement,
collision, audio, and persistence remain unverified.

The executable references `tex_menu.tga`, but the decrypted bundle only has
`tex_menu1.tga` and `tex_ig.tga`. An isolated alias experiment did not change
the state and produced no guest request for the missing filename, so the asset
is a preservation gap but is not yet proven to be the transition blocker. See
[`20260827_pacman_name_entry_probe.md`](../game_tests/20260827_pacman_name_entry_probe.md)
for the exact route, captures, and start-gate fault evidence.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
