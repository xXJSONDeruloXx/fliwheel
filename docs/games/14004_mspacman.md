# Ms. Pac-Man (Bundle 14004)

**Status:** 🟡 NORMAL PATH REACHES CONTROLLABLE STAGE 1 + COLLISION/LIFE + HEADED WAV SINK | **Evidence:** the four measured async-completion contracts now run by default and reach name entry, the main menu, Play Game/tutorial, and a controllable Stage 1 maze; one guest collision produced `die.wav` and a visible life decrement/reset, while all 20 WAV sources map and the headed sink accepted the observed gameplay events; persistence, full-content, physical-mixer, and long-run parity remain open | **Engine:** Tetris Runtime

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
- The maze UV matcher now accepts the guest's observed one-pixel integer edge
  convention for the 256×256 `tex_maze_blue.bin` upload; the focused route
  records zero triangle-strip upload skips through guest frame 749
- The normal path now reaches the full maze/HUD and advances the score while
  driving the clickwheel quadrant packet; a focused follow-up maps all 20 WAV
  sources and dispatches the observed Stage 1 events through the headed sink
- A bounded route also reaches a guest collision/death transition and returns
  to `READY!` with the life counter decremented
- This is not yet a full gameplay, physical-mixer, persistence, full-content,
  or long-run certification
- The four measured async-completion contracts are title-scoped and enabled by
  default for this bundle

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```

See the [2026-08-27 gameplay probe](../game_tests/20260827_mspacman_gameplay_probe.md)
and [2026-08-28 maze UV-edge probe](../game_tests/20260828_mspacman_uv_edge_probe.md)
for the exact commands and captured evidence.
