# iPod Clickwheel Games — Clicky Emulator

Game-by-game compatibility and launch documentation. The current authority is
the [2026-08-26 interactive matrix](../game_tests/20260826_interactive_matrix.md);
older pages retain historical investigation notes and are not completion
claims.

## Quick Reference

| Game | Bundle | Current state | Docs |
|------|---------|---------------|------|
| Tetris | 66666 | 🟡 Content-level interaction partial | [→](66666_tetris.md) |
| Cubis 2 | 99999 | ❌ Asset/renderer blocked | [→](99999_cubis2.md) |
| Texas Hold'em | 33333 | 🟡 Default loading-only; scoped experiment reaches partial first scene | [→](33333_holdem.md) |
| Ms. PAC-MAN | 14004 | 🟡 Loading screen only | [→](14004_mspacman.md) |
| PAC-MAN | AAAAA | 🟡 Loading screen only | [→](AAAAA_pacman.md) |
| Mahjong | 77777 | 🟡 Texture/UV partial | [→](77777_mahjong.md) |
| Mini Golf | 88888 | 🟡 Loading/progress only | [→](88888_minigolf.md) |
| The Sims Bowling | 1500C | ❌ Renderer/asset decode blocked | [→](1500C_simsbowling.md) |
| The Sims Pool | 1500E | ❌ Renderer/asset decode blocked | [→](1500E_simspool.md) |
| Sudoku | 50513 | 🟡 Input lifecycle + menu/exit path verified; board not reached | [→](50513_sudoku.md) |
| Royal Solitaire | 50514 | 🟡 UV/quad corruption | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |
| Bejeweled | 55555 | 🟡 PopCap partial | [→](55555_bejeweled.md) |
| Zuma | 44444 | 🟡 PopCap partial | [→](44444_zuma.md) |
| Vortex | 12345 | 🟡 Animated splash/VBO open | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |
| iQuiz | 11002 | ❌ Pack/content loading blocked | [→](11002_twa.md) |
| SAT Prep Reading | 11050 | 🟡 Splash/loading only | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |
| SAT Prep Writing | 11051 | 🟡 Splash/loading only | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |
| SAT Prep Mathematics | 11052 | 🟡 Splash/loading only | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |
| LOST | 1B200 | ❌ Shader/render-server blocked | [→](1B200_lost.md) |
| musika | 1C300 | 🟡 Splash only | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |

**Summary:** All 20 decrypted bundles launch far enough for a controlled
interactive probe, but none is yet certified fully playable. Tetris is the
furthest along; Hold'em's default path runs without a fatal but remains at
loading, while a title-scoped experiment now reaches an incomplete first game
scene. The remaining titles need content-specific input and renderer/asset
fixes. The interactive matrix is the current default-contract status source.

Latest interactive reports: `/tmp/fliwheel_interactive_full_{a,b,c,d}/interactive_matrix.md`.

## Running Games

### Rendering-smoke Games

These launch scripts exercise the rendering HLE and startup path; “working” in
older notes does not mean input, persistence, gameplay, and sound are all
verified.

```bash
./scripts/tetris.sh                # most tested
./scripts/cubis2.sh                # highest draw count
./scripts/holdem.sh                # complex poker game
./scripts/mspacman.sh              # classic arcade
./scripts/pacman.sh                # classic arcade
./scripts/mahjong.sh               # tile matching
./scripts/minigolf.sh              # golf game
./scripts/simsbowling.sh           # bowling sim
./scripts/simspool.sh              # pool sim
```

### PopCap / legacy partial-render Games

PopCap engine games still need a content-level renderer regression:

```bash
./scripts/bejeweled.sh             # startup/partial renderer probe
./scripts/zuma.sh                  # startup/partial renderer probe
```

### Sudoku / Solitaire

Sudoku and Royal Solitaire use normalized coordinates and run directly:

```bash
./target/release/eapp /path/to/Games_RO/50513 --headless
./target/release/eapp /path/to/Games_RO/50514 --headless
```

### Common Script Options

```bash
./scripts/<game>.sh --timeout 15    # auto-terminate after 15s
./scripts/<game>.sh --headless      # no window (CI / testing)
./scripts/<game>.sh --verbose       # debug-level logging
./scripts/<game>.sh --dump 30       # dump first 30 frames as PPM
./scripts/<game>.sh --no-build      # skip cargo build
./scripts/<game>.sh --no-capture    # skip PPM frame captures
```

### Required Environment

All games require the experimental GL HLE renderer:

```bash
export CLICKY_EXPERIMENTAL_GL_HLE=1
export CLICKY_GL_GATE_B=1
export CLICKY_GL_LIVE_CONTINUOUS=1
export CLICKY_GL_PRESENT_VFLIP=1
```

Vflip is **auto-suppressed** for the currently identified normalized-coordinate
engines (Sims Bowling/Pool, Sudoku, and Solitaire). Launch scripts set these
automatically where they are current.

### Bundle Directory

The current decrypted corpus used for regression is at:
```
/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/<bundle_id>/
```

For a portable corpus-wide probe, pass the root explicitly:
```bash
./scripts/test_decrypted_games_interactive.sh /path/to/Games_RO 66666
```

## Engine Classification

| Engine | Games | Coords | Vflip | Frame Begin | Assets |
|--------|-------|--------|-------|-------------|--------|
| Tetris-style Runtime | Tetris, Cubis 2, Mini Golf, Mahjong, Ms. Pac-Man, Pac-Man | Pixel | Yes | ordinal-158 | .pix |
| Sims/Rserver Runtime | Sims Bowling, Sims Pool | **NDC observed** | Yes | ordinal-158 | .rlb + rserver |
| Hold'em Runtime | Texas Hold'em | Pixel | Yes | ordinal-158 | .ipd/.blob |
| Sudoku/SS Engine | Sudoku, Solitaire | **NDC** | **No** | **Auto** | Minimal |
| PopCap Engine | Zuma, Bejeweled | Pixel | Yes | ordinal-158 | **DMA** + .ipd |
| iQuiz Engine | TWA/iQuiz | Pixel | Yes | ordinal-158 | .ipd (AsyncFileIO:7) |
| Lost Engine | Lost | Pixel | Yes | ordinal-158 | rserver.bin (shaders) |

## Blocked Games

### TWA/iQuiz — Pack content loading
- AsyncFileIO:7 directory enumeration ✅ works
- Game opens `Sounds/All_Out.wav` via Filesytem:0 ✅ works
- After dir callback, game requests `"data"` file — a generated pack index
  that doesn't exist in the preservation bundle
- Pack icon textures (149×75 A8) never loaded — game binds material
  handle 0x28 but uploads go to different GL names
- Need: pack metadata generation + texture loading pipeline

### Lost — Programmable shader pipeline
- Loads `rserver.bin` (105KB) via AsyncFileIO:3
- Calls OpenGLES:164 (shader create) with pointer to shader binary
- Ordinals 164/167/152/153 stubbed but game needs real shader execution
- Frame loop: clear → bind → present (0 draws)
- Need: shader binary parser + compiler/interpreter for rserver.bin format

### Solitaire — UV/quad corruption
- The title advances beyond its splash, but current captures have large
  corrupted quads and long zero-draw periods.
- Need an atlas/material regression before calling the board correct.

### Vortex — VBO indirection / splash only
- ordinal-175/125 VBO setup corrupts vertex array definitions
- Need: pointer-to-struct dereferencing for ordinal-137
- The interactive matrix reaches animated title art but not a verified game
  scene.

## Recent Changes

### 2026-06-26: AsyncFileIO:7, shader stubs, 12/16 showing content
- AsyncFileIO:7 directory enumeration with async callback protocol
- OpenGLES:164/167/152/153 shader program API stubs for Lost
- Filesytem:0 path reading — TWA opens `Sounds/All_Out.wav`
- Ordinal-4 texture name capture fallback
- Ordinal-45 descriptor diagnostic for deferred texture descriptors
- TWA root cause: pack content loading pipeline, not dir enumeration
- Lost root cause: programmable GPU pipeline, not missing ordinals

### 2026-08-26: Reversed-register input lifecycle
- Sudoku/Solitaire-family `InputEvents:0` owners are retained across the
  wrapper's dropped register and cleared one poll later, preventing stale
  event heads without changing Tetris's owner path
- Sudoku's Menu edge is confirmed as teardown/exit; the puzzle-start event and
  post-RLB scene contract remain open

### 2026-06-26: Sudoku works, PopCap DMA, 12/16 games rendering
- Sudoku: auto-begin, NDC scaling, auto-vflip, 0-draw preservation
- Bejeweled/Zuma: DMA framebuffer overlay injection + alpha blending
- Per-game launch scripts (11 scripts)
- Per-game documentation (14 docs + index)

### 2026-06-25: Initial compatibility, HW stub, 10/16 working
- 9/16 games working, HW stub for 0x14000000 region
- Filesytem handler, per-game scripts and docs

## See Also

- [2026-08-26 interactive decrypted matrix](../game_tests/20260826_interactive_matrix.md)
- [2026-08-25 Sudoku input regression](../game_tests/20260825_sudoku_input.md)
- [2026-08-26 Sudoku event lifecycle](../game_tests/20260826_sudoku_event_lifecycle.md)
- [Compatibility Report](../game_tests/20260625_compatibility_report.md) — full metrics
- [Debug Analysis](../game_tests/debug_analysis.md) — root cause analysis
- [EAPP Format Specification](../EAPP_FORMAT_SPECIFICATION.md)
- [Emulator Architecture](../EMULATOR_ARCHITECTURE.md)
