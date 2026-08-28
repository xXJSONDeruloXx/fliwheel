# Pac-Man (Bundle AAAAA)

**Status:** 🟡 DIAGNOSTIC GAMEPLAY + HEADED AUDIO | **Evidence:** the normal path reaches a rendered, controllable Stage 1 maze and the headed WAV sink accepts gameplay events; physical mixer parity, collisions/lives, and persistence remain open | **Engine:** Tetris Runtime

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

The PAC-MAN TGA callback now receives the request status and byte count that
the resource manager expects, and the guest's own TGA parser populates the
texture dimensions on the normal path. The 2026-08-28 probe renders a stable
33-37-draw maze from frame 775 through frame 1048. Captures show `READY`,
moving Pac-Man and animated ghosts; the tested wheel route moves the player
and advances the score from `0` at frame 800 to `30` at frame 880 and `40` at
frame 890, with no fatal signature through the bounded run.
Audio assets and guest audio events were observed during the headless probe. A
matching headed replay emitted 12 mapped events and 12 `played sound` receipts
through the desktop WAV sink, with no decoder or sink errors. Physical speaker
output, overlap/mixing, volume controls, and timing parity remain unverified.

Before the measured TGA completion contract was promoted, the default
`START GAME` route advanced through initialization states 2 through 9 and
faulted before the first maze frame at `PC 0x1801628c` while reading `0x58`
through a null nested pointer. The retained pre-promotion evidence showed the
experimental live-GL path faulting at frame 785 and the legacy path reaching
the same guest fault at frame 791; that boundary was not renderer-specific.

The executable references `tex_menu.tga`, but the decrypted bundle only has
`tex_menu1.tga` and `tex_ig.tga`. An isolated alias experiment did not change
the state and produced no guest request for the missing filename, so the asset
is a preservation gap but is not yet proven to be the transition blocker. See
[`20260827_pacman_name_entry_probe.md`](../game_tests/20260827_pacman_name_entry_probe.md)
for the exact menu route and pre-fix start-gate evidence. The opt-in maze and
input result is recorded in
[`20260828_pacman_gameplay_probe.md`](../game_tests/20260828_pacman_gameplay_probe.md).

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```
