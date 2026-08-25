# Mini Golf (Bundle 88888)

**Status:** ✅ WORKS | **Draws:** 11,166 (10s) | **Engine:** Tetris Runtime

## Quick Start
```bash
./scripts/minigolf.sh
./scripts/minigolf.sh --timeout 15
./scripts/minigolf.sh --headless
```

## Bundle Info
- **Executable:** `Minigolf_1_1_2563296.bin` (eapp format)
- **Asset Format:** `.wav` (30 files) + `.da` course data

## Assets
- **Audio:** 30 `.wav` sound effect files
- **Courses:** `c00/` with localized course data (.da, .de, .en, .es, etc.)
- **Music:** `.m4a` background tracks

## Notable
- Multiple course directories with localization support
- Uses `.da` course data files (likely per-course layout)
- Save files: `game.sav`, `prefs.sav` with MGCT/RPCT headers

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
