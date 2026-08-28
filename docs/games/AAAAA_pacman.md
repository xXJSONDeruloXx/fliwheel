# Pac-Man (Bundle AAAAA)

**Status:** 🟡 PARTIAL NAME/BOARD SCENE | **Evidence:** default resource loading completes and positive wheel input commits name-entry characters, but the confirm/menu/maze transition remains open | **Engine:** Tetris Runtime

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
- Positive wheel motion commits characters in name entry; the confirm control and
  main-menu/maze transition are not confirmed

## Current evidence

The normal path completes 20 asynchronous resource requests, renders the
Namco/name-entry scene, and receives both wheel and Select edges. A bounded
positive-wheel route commits `A`, `AB`, `ABG`, and `ABGL`, then continues to an
eight-character name with the next-character cursor active. The same route does
not reach the main menu or maze.

The executable references `tex_menu.tga`, but the decrypted bundle only has
`tex_menu1.tga` and `tex_ig.tga`. An isolated alias experiment did not change
the state and produced no guest request for the missing filename, so the asset
is a preservation gap but is not yet proven to be the transition blocker. See
[`20260827_pacman_name_entry_probe.md`](../game_tests/20260827_pacman_name_entry_probe.md)
for the exact route and captures.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
