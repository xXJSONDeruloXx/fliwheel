# iPod Clickwheel Games — Clicky Emulator

Game-by-game compatibility and launch documentation.

## Quick Reference

| Game | Bundle | Script | Status | Docs |
|------|---------|--------|--------|------|
| Tetris | 66666 | `./scripts/tetris.sh` | ✅ WORKS | [→](66666_tetris.md) |
| Cubis 2 | 99999 | `./scripts/cubis2.sh` | ✅ WORKS | [→](99999_cubis2.md) |
| Texas Hold'em | 33333 | `./scripts/holdem.sh` | ✅ WORKS | [→](33333_holdem.md) |
| Ms. Pac-Man | 14004 | `./scripts/mspacman.sh` | ✅ WORKS | [→](14004_mspacman.md) |
| Pac-Man | AAAAA | `./scripts/pacman.sh` | ✅ WORKS | [→](AAAAA_pacman.md) |
| Mahjong | 77777 | `./scripts/mahjong.sh` | ✅ WORKS | [→](77777_mahjong.md) |
| Mini Golf | 88888 | `./scripts/minigolf.sh` | ✅ WORKS | [→](88888_minigolf.md) |
| Sims Bowling | 1500C | `./scripts/simsbowling.sh` | ✅ WORKS | [→](1500C_simsbowling.md) |
| Sims Pool | 1500E | `./scripts/simspool.sh` | ✅ WORKS | [→](1500E_simspool.md) |
| Sudoku | 50513 | — | ✅ WORKS | [→](50513_sudoku.md) |
| Bejeweled | 55555 | `./scripts/bejeweled.sh` | ✅ DMA | [→](55555_bejeweled.md) |
| Zuma | 44444 | `./scripts/zuma.sh` | ✅ DMA | [→](44444_zuma.md) |
| Solitaire | 50514 | — | ⚠️ UV skip | — |
| Vortex | 12345 | — | ⚠️ VBO | — |
| TWA/iQuiz | 11002 | — | ❌ Pack load | [→](11002_twa.md) |
| Lost | 1B200 | — | ❌ Shader | [→](1B200_lost.md) |

**Summary:** 10 fully working + 2 DMA working = 12/16 showing content. 2 partial. 2 blocked.

## Running Games

### Working Games (10)

Each working game has a launch script in `scripts/`:

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

### DMA Background Games (2)

PopCap engine games render background via DMA framebuffer:

```bash
./scripts/bejeweled.sh             # 98% content (gem sprites + game board)
./scripts/zuma.sh                  # 33% content (game board top portion)
```

### Sudoku

Sudoku uses NDC coordinates and runs directly (no script yet):
```bash
./target/release/eapp /path/to/Games_RO/50513
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

Vflip is **auto-suppressed** for NDC-coordinate engines (Sudoku/Solitaire).
Launch scripts set these automatically.

### Bundle Directory

Games live in the preservation dump at:
```
~/Downloads/16-ipod-games/Games_RO/<bundle_id>/
```

Override with environment variables:
```bash
TETRIS_BUNDLE=/path/to/66666 ./scripts/tetris.sh
```

## Engine Classification

| Engine | Games | Coords | Vflip | Frame Begin | Assets |
|--------|-------|--------|-------|-------------|--------|
| Tetris Runtime | Tetris, Cubis 2, Mini Golf, Mahjong, Ms. Pac-Man, Pac-Man, Sims Bowling, Sims Pool | Pixel | Yes | ordinal-158 | .pix |
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

### Solitaire — UV mismatch (~5 skips/frame)
- `select_smallest_containing_upload` doesn't match 24×40 UV span
  within a 577×40 atlas for some glyphs
- 93% content rendered, minor visual artifacts

### Vortex — VBO indirection
- ordinal-175/125 VBO setup corrupts vertex array definitions
- Need: pointer-to-struct dereferencing for ordinal-137
- Only renders title graphic (~18% content)

## Recent Changes

### 2026-06-26: AsyncFileIO:7, shader stubs, 12/16 showing content
- AsyncFileIO:7 directory enumeration with async callback protocol
- OpenGLES:164/167/152/153 shader program API stubs for Lost
- Filesytem:0 path reading — TWA opens `Sounds/All_Out.wav`
- Ordinal-4 texture name capture fallback
- Ordinal-45 descriptor diagnostic for deferred texture descriptors
- TWA root cause: pack content loading pipeline, not dir enumeration
- Lost root cause: programmable GPU pipeline, not missing ordinals

### 2026-06-26: Sudoku works, PopCap DMA, 12/16 games rendering
- Sudoku: auto-begin, NDC scaling, auto-vflip, 0-draw preservation
- Bejeweled/Zuma: DMA framebuffer overlay injection + alpha blending
- Per-game launch scripts (11 scripts)
- Per-game documentation (14 docs + index)

### 2026-06-25: Initial compatibility, HW stub, 10/16 working
- 9/16 games working, HW stub for 0x14000000 region
- Filesytem handler, per-game scripts and docs

## See Also

- [Compatibility Report](../game_tests/20260625_compatibility_report.md) — full metrics
- [Debug Analysis](../game_tests/debug_analysis.md) — root cause analysis
- [EAPP Format Specification](../EAPP_FORMAT_SPECIFICATION.md)
- [Emulator Architecture](../EMULATOR_ARCHITECTURE.md)
