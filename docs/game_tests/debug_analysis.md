# iPod Game Debug Analysis - Clicky Emulator

**Date:** 2026-06-25  
**Updated:** With crash analysis and revised root causes  
**Scope:** 16 games tested with experimental GL HLE

---

## Compatibility Matrix (Final)

| Bundle | Game | Status | Draws | Skips | Uploads | Crash/Issue |
|--------|------|--------|-------|-------|---------|-------------|
| 11002 | TWA/iQuiz | ❌ NO_RENDER | 0 | 8,273 | 1 | `Filesytem` import unhandled |
| 12345 | Vortex | ⚠️ PARTIAL | 184 | 279 | 7 | Limited texture uploads |
| 14004 | Ms. Pac-Man | ✅ WORKS | 26,386 | 121 | 1 | — |
| 1500C | Sims Bowling | ✅ WORKS | 3,272 | 120 | 1 | — |
| 1500E | Sims Pool | ✅ WORKS | 3,403 | 120 | 2 | — |
| 1B200 | Lost | ❌ NO_RENDER | 0 | 0 | 2 | ARM divide-by-zero |
| 33333 | Texas Hold'em | ✅ WORKS | 33,851 | 120 | 9 | — |
| 44444 | Zuma | 💥 CRASH | 42 | 6 | 5 | `FatalMemException` at 0x1400000c |
| 50513 | Sudoku | 👀 BOOTS | 2 | 1 | 1 | Minimal rendering |
| 50514 | Solitaire | ⚠️ PARTIAL | 343 | 121 | 5 | No crash, limited |
| 55555 | Bejeweled | 💥 CRASH | 180 | 5 | 8 | `FatalMemException` at 0x1400000c |
| 66666 | Tetris | ✅ WORKS | 10,572 | 120 | 38 | — |
| 77777 | Mahjong | ✅ WORKS | 21,587 | 121 | 0 | — |
| 88888 | Mini Golf | ✅ WORKS | 11,392 | 122 | 0 | — |
| 99999 | Cubis 2 | ✅ WORKS | 43,963 | 120 | 57 | — |
| AAAAA | Pac-Man | ✅ WORKS | 24,876 | 123 | 1 | — |

**Summary:** 9 WORKS / 2 PARTIAL / 1 BOOTS / 2 CRASH / 2 NO_RENDER

---

## Root Cause #1: Unmapped Memory Write at 0x1400000c (2 games)

**Games:** Zuma (44444), Bejeweled (55555)

### Evidence
```
FatalMemException {
    pc: 0x18001720,
    access: Write,
    offset: 0x1400000c,
    in_device: "eapp, <unmapped>",
}
```

Both games render successfully (42 and 180 draws respectively) then crash when they try to write to address `0x1400000c`.

### Analysis
- Address `0x1400000c` is in the gap between work RAM (`0x10000000..0x14000000`) and image RAM (`0x18000000..`)
- On real iPod hardware, this region contains **hardware registers** (DMA controller, display FIFO, etc.)
- The crash occurs after several rendered frames, suggesting the game enters a state transition
- Both games hit a tight loop at PC `0x1800172x` before the fault

### Fix
Map or stub the `0x14000000` memory region in `EappBus`. Likely just needs a write-ignore (stub) since this is a hardware register write, not actual memory.

---

## Root Cause #2: Unhandled `Filesytem` Import Module (1 game)

**Game:** TWA/iQuiz (11002)

### Evidence
```
WARN EAPP_IMPORT > unhandled module Filesytem
```

Note the typo — `Filesytem` not `Filesystem` — this is the actual module name in the binary.

### Analysis
- TWA uses a different resource loading mechanism than Tetris
- It has 126 `.ipd` files and 26 `.tga` files for textures
- The `Filesytem:0` call is likely how the game opens resource files
- Without this handler, TWA can never load its textures (8,273 skipped draws)
- Only 1 texture upload (2x2 A8 placeholder) occurs before rendering starts

### Import modules for TWA:
```
AsyncFileIO, Audio, Filesytem, InputEvents, Metadata, miscTBD, OpenGLES, Settings
```

### Fix
Add a `Filesytem` import handler in `mod.rs` that maps filesystem operations to host file I/O. At minimum, ordinal 0 needs to return a valid result so the game can proceed to load resources.

---

## Root Cause #3: ARM CPU Divide-by-Zero (1 game)

**Game:** Lost (1B200)

### Evidence
```
svc: putchar 'Arithmetic exception: Divide By Zero'
```

The game prints this message character-by-character via SVC before halting.

### Sequence
1. Loads `rserver.bin` (105,020 bytes) via AsyncFileIO
2. Initializes audio subsystem (ordinal 51/52/55/47/48/49/60)
3. Crashes during or after audio init with divide-by-zero

### Analysis
- Lost uses a unique engine that loads `rserver.bin` (likely a resource server)
- Audio uses LuminanceAlpha88 texture format
- The divide-by-zero may be caused by an unemulated audio/timing register returning 0
- Lost's entry function returns different values than Tetris (different runtime)

### Fix
- Short-term: Add ARM UDIV/SDIV exception handling (return 0 instead of crashing)
- Long-term: Find the uninitialized variable causing the zero divisor

---

## Root Cause #4: Limited Texture Uploads (3 games)

**Games:** Vortex (12345), Solitaire (50514), Sudoku (50513)

### Analysis
These games render but are limited by:
- **Vortex**: Only 7 texture uploads, 184 draws, 279 skips — runs indefinitely
- **Solitaire**: 5 uploads, 343 draws, 121 skips — runs indefinitely
- **Sudoku**: 1 upload (1x1 placeholder), 2 draws — barely renders

These games use different resource paths. The textures they need may be loaded through paths the emulator doesn't intercept (similar to TWA).

### Fix
- More `AsyncFileIO` ordinal coverage
- Alternative texture loading path detection

---

## New Discovery: Different Game Engine Families

### Engine A: Tetris Runtime (most working games)
- Entry function returns specific game object pointers
- Loads `.pix` files via `AsyncFileIO:3`
- Handles `OpenGLES:37` mode=7 quads
- Games: Tetris, Cubis 2, Mini Golf, Mahjong, Ms. Pac-Man, Pac-Man, Sims Bowling, Sims Pool

### Engine B: Hold'em Runtime (partially working)
- Loads `.ipd` files via `AsyncFileIO:3`
- Uses `AsyncFileIO:2` for secondary callbacks
- Games: Texas Hold'em

### Engine C: iQuiz Runtime (failing)
- Uses `Filesytem` import module
- Loads `.ipd` and `.blob` files
- Different file loading path
- Games: TWA/iQuiz

### Engine D: Lost Runtime (crashing)
- Loads `rserver.bin` as resource server
- Uses LuminanceAlpha88 texture format
- Games: Lost

### Engine E: PopCap Runtime (crashing at 0x1400)
- Renders but crashes writing to `0x1400000c`
- Uses `.ipd` and `.blob` files
- Games: Zuma, Bejeweled

---

## Recommended Fix Priority

| Priority | Fix | Games Fixed | Difficulty |
|----------|-----|-------------|------------|
| **HIGH** | Map/stub `0x14000000` region | Zuma, Bejeweled | Easy |
| **HIGH** | Handle `Filesytem` import | TWA/iQuiz | Medium |
| **MEDIUM** | Handle ARM UDIV exception | Lost | Medium |
| **MEDIUM** | More AsyncFileIO coverage | Vortex, Solitaire, Sudoku | Medium |

**Potential outcome:** 9 → 11 games working with just 2 fixes (stub 0x1400 + Filesytem handler)

---

## Test Environment

```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
RUST_LOG=EAPP_GL=info,EAPP_IMPORT=info
```

**Platform:** Apple Silicon macOS (aarch64)  
**Emulator:** clicky-eapp v0.1.0  
**Build:** `target/release/eapp`

---

*Updated: 2026-06-25 with crash analysis and engine classification*

---

## Post-Fix Update (2026-06-25)

### HW Stub Fix (committed)
- Added `HW_STUB_BASE` (0x14000000) with 64MiB stub region
- Prevents FatalMemException crash in Zuma and Bejeweled
- Both games now gracefully exit with "Abnormal termination" instead of crashing
- Games still don't progress because DMA registers need proper response simulation

### Filesytem Handler Fix (committed)
- Added `handle_filesystem_import()` for the `Filesytem` module
- Returns 1 for ordinal 0 (init/open success)
- TWA still shows 0 rasterized draws because .ipd texture files need AsyncFileIO:7 directory enumeration support

### Remaining Issues
1. **Zuma/Bejeweled DMA**: Write to 0x1402_000c, then reads back waiting for completion. Need DMA controller emulation or at least "complete" status bit.
2. **TWA .ipd loading**: Needs AsyncFileIO:7 to enumerate directory contents and return file lists
3. **Lost divide-by-zero**: ARM exception during audio init
4. **Sudoku/Bejeweled partial**: Need more texture loading paths

### Compatibility After Fixes

| Bundle | Draws | Change | Status |
|--------|-------|--------|--------|
| 11002 | 0 | same | TWA - needs .ipd loading via AsyncFileIO:7 |
| 12345 | 99 | ~same | Vortex - partial renders |
| 14004 | 26,405 | same | Ms. Pac-Man - WORKS |
| 1500C | 3,296 | same | Sims Bowling - WORKS |
| 1500E | 3,397 | same | Sims Pool - WORKS |
| 1B200 | 0 | same | Lost - divide-by-zero crash |
| 33333 | 34,088 | same | Texas Hold'em - WORKS |
| 44444 | 42 | no crash! | Zuma - DMA spin-wait (was FatalMem) |
| 50513 | 2 | same | Sudoku - minimal renders |
| 50514 | 346 | same | Solitaire - partial renders |
| 55555 | 180 | no crash! | Bejeweled - DMA spin-wait (was FatalMem) |
| 66666 | 10,840 | same | Tetris - WORKS |
| 77777 | 21,235 | same | Mahjong - WORKS |
| 88888 | 11,166 | same | Mini Golf - WORKS |
| 99999 | 42,990 | same | Cubis 2 - WORKS |
| AAAAA | 24,511 | same | Pac-Man - WORKS |

---

## 2026-06-26 Update: Sudoku Fixed, Three New Bugs Found and Fixed

### Bug 1: Present Without Active Frame (Sudoku/SS Engine)
**Symptom:** Sudoku rendered 1 rasterized draw, then all subsequent frames showed
`candidate_present without active frame; discarded`.

**Root Cause:** The Sudoku/Solitaire engine never calls ordinal-158 (frame begin).
Its per-frame loop is: `OpenGLES:159 (bind) → 149 (setup) → 157 (present)`.
The emulator required ordinal-158 to start a frame, so every present after the
first was discarded.

**Fix:** Auto-begin a frame when ordinal-157 arrives with no active frame,
matching the existing pattern for auto-begin on draw calls. Also preserve
the previous framebuffer content for 0-draw idle frames instead of
overwriting with black.

### Bug 2: NDC Coordinates → 1 Pixel of Coverage
**Symptom:** Sudoku's draw showed `cov=1` (1 pixel of rasterization) despite
being a fullscreen 320×240 quad.

**Root Cause:** Sudoku passes vertex positions in normalized device coordinates
like `(0,0)→(1.2,0.9)`. The Tetris engine uses pixel coordinates `(0,0)→(320,239)`.
The rasterizer iterates `min_y..=max_y` clamped to `(0..239)`, so coordinates
in the 0–1 range only covered 1 pixel row.

**Fix:** Detect NDC coordinates (max_coord < 2.0) and apply viewport-style
scaling: map `(min_x..max_x, min_y..max_y)` → `(0..FB_WIDTH, 0..FB_HEIGHT)`.
This both fills the screen and correctly centers the content.

Initially used `x * 320, y * 240` which extended 1.2→384 pixels past the screen
edge, causing the image to be shifted 30px right. Viewport-style scaling
fixed the centering.

### Bug 3: Upside-Down Image (Vflip on NDC Frames)
**Symptom:** Sudoku splash appeared vertically flipped.

**Root Cause:** `CLICKY_GL_PRESENT_VFLIP=1` flips the framebuffer vertically,
which is needed for pixel-coord engines (Tetris renders bottom-to-top) but
incorrect for NDC engines (Sudoku renders top-to-bottom).

**Fix:** Added `ndc_frame: bool` field to `LiveGl`, set when NDC scaling is
applied. The vflip in `complete_frame()` and `present()` is suppressed when
`ndc_frame` is true.

### Impact
- Sudoku: 0% → 19% framebuffer content, 120 presented frames, properly
  centered and right-side-up
- Solitaire: 0 → 120 presented frames (idle loop now works)
- Zero regressions in 9 previously working games
- All 16 unit tests pass
