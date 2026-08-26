# iPod Game Compatibility Report — Clicky Emulator

**Date:** 2026-06-26 (final)  
**Test Method:** Headless 5s + headed smoke tests, experimental GL HLE enabled  
**Build:** `clickwheel-games` branch, commit `d679e12`

## Summary

| Category | Count | Games |
|----------|-------|-------|
| ✅ **Fully Working** | 10 | Tetris, Cubis 2, Hold'em, Ms. Pac-Man, Pac-Man, Mahjong, Mini Golf, Bowling, Pool, Sudoku |
| ✅ **DMA Background** | 2 | **Bejeweled** (98% content), **Zuma** (33% content = top game board) |
| ⚠️ **Partial (was VBO-broken)** | 1 | **Vortex** (59→9096 draws! All 3 per-frame draws now render) |
| ⚠️ **Minor UV skip** | 1 | Solitaire (~5 UV skips per 8s, 99% content) |
| 🔴 **Not Working** | 2 | TWA (pack content loading), Lost (programmable shader pipeline) |

**13 out of 16 games show visual content on screen.** (Up from 12)

## Detailed Results (5s headless)

| Bundle | Game | Draws | Status | Notes |
|--------|------|-------|--------|-------|
| 66666 | Tetris | 4,077 | ✅ | Reference game, fully playable |
| 99999 | Cubis 2 | 24,269 | ✅ | Highest draw count |
| 33333 | Hold'em | 18,550 | ✅ | Complex poker AI |
| 14004 | Ms. Pac-Man | 16,250 | ✅ | Procedural maze |
| AAAAA | Pac-Man | 15,816 | ✅ | Classic arcade |
| 77777 | Mahjong | 13,047 | ✅ | Tile matching |
| 88888 | Mini Golf | 7,014 | ✅ | Course data |
| 1500C | Sims Bowling | 1,484 | ✅ | Physics engine |
| 1500E | Sims Pool | 1,954 | ✅ | Physics engine |
| 50513 | Sudoku | 2 | ✅ | NDC engine, splash centered |
| 55555 | **Bejeweled** | 180 | ✅ | DMA bg 98% + gem sprites overlay |
| 44444 | **Zuma** | 42 | ✅ | DMA bg 33% (top game board only) |
| 12345 | **Vortex** | 9,096 | ⚠️ | **WAS 59, NOW 9096** — VBO fix! |
| 50514 | Solitaire | 112 | ⚠️ | ~5 UV skips/8s, 99% content |
| 11002 | TWA | 0 | 🔴 | AsyncFileIO:7 works, pack loading blocked |
| 1B200 | Lost | 0 | 🔴 | Shader pipeline needs real GPU execution |

## Playing Games

All working games are **playable in headed mode**! The keyboard maps to click wheel:

| Key | iPod Control |
|-----|-------------|
| ↑ ↓ ← → | Click wheel (scroll / direction) |
| Enter | Select (center button) |
| M | Menu |
| Mouse wheel | Scroll |

Launch with a game script:
```bash
./scripts/tetris.sh          # headed, with GL rendering
./scripts/tetris.sh --headless  # headless test
```

## Key Fix: Vortex VBO Component Count (59→9096 draws)

**Root cause:** When a VBO is active (ordinals 175/125), the game modifies the vertex array descriptor struct in guest memory. The next ordinal-137 call reads this struct and passes a VBO offset/pointer as `component_count` instead of a real component count (e.g. `0x10040040` instead of `4`). This caused `live_decode_positions_range()` to reject the position array as "unusable".

**Fix:** In `live_handle_array_def()`, detect VBO-mode component counts (value > 32) and infer the real count from format+stride. For GL_FIXED with stride=40: total components = 40/4 = 10, position array gets 4 components (x,y,z,w).

## Fixes Applied This Session

1. **Vortex VBO fix** — component count inference: detect >32, derive from format+stride
2. **OpenGLES:164/167/152/153** — shader program API stubs for Lost
3. **AsyncFileIO:7** — directory enumeration with async callback for TWA
4. **Filesytem:0** — path reading + file tracking (TWA opens `Sounds/All_Out.wav`)
5. **OpenGLES:4** — texture name capture from glBindTexture
6. **Ordinal-45 diagnostic** — descriptor logging for deferred texture descriptors
7. **PPM dump filenames** — use game_id instead of hardcoded "tetris"

## Engine Classification

| Engine | Games | Background | Coordinates | Vflip | VBO |
|--------|-------|------------|-------------|-------|-----|
| Tetris Runtime | 9 | GL texture | Pixel | Yes | No |
| Hold'em Runtime | Hold'em | GL texture | Pixel | Yes | No |
| Sudoku/SS Engine | Sudoku, Solitaire | GL texture | NDC | No | No |
| PopCap Engine | Bejeweled, Zuma | **DMA buffer** | Pixel | Yes | No |
| iQuiz Engine | TWA | .ipd (needs packs) | Pixel | Yes | No |
| **Lost Engine** | Lost | rserver.bin (shader) | Pixel | Yes | No |
| **Vortex Engine** | Vortex | GL texture | Pixel | Yes | **Yes** |
