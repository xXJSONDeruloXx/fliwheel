# Mahjong (Bundle 77777)

**Status:** 🟡 READABLE TITLE ART + RLB STREAM PARTIAL | **Evidence:** opt-in stream/format probe matches the reference title art; no board or full-play receipt yet | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/games/mahjong.sh
./scripts/games/mahjong.sh --timeout 15
./scripts/games/mahjong.sh --headless
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
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
FLIWHEEL_MAHJONG_ASYNC0_COMPLETE=1
FLIWHEEL_MAHJONG_ASYNC2_COMPLETE=1
```

The stream and format milestone is recorded in
[the 2026-08-29 test note](../game_tests/20260829_mahjong_stream_and_format.md).
These completion flags remain opt-in until the menu, board controls, audio,
and save/persistence paths have each been verified.
