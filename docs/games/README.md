# Decrypted iPod clickwheel games

Game-by-game compatibility and launch documentation for fliwheel. The current
authority is
the [2026-08-26 interactive matrix](../game_tests/20260826_interactive_matrix.md);
older pages retain historical investigation notes and are not completion claims.

## Quick Reference

| Game | Bundle | Current state | Docs |
|------|---------|---------------|------|
| Tetris | 66666 | 🟡 Core controls + centered board; gameplay parity open | [→](66666_tetris.md) |
| Cubis 2 | 99999 | 🟡 Main menu + board rendering and effect WAV sink path verified; matching, music, and save open | [→](99999_cubis2.md) |
| Texas Hold'em | 33333 | 🟡 Default route reaches coherent poker table; hand flow, sound, and save open | [→](33333_holdem.md) |
| Ms. PAC-MAN | 14004 | 🟡 Normal-path Stage 1 + collision/life + headed WAV sink; full play open | [→](14004_mspacman.md) |
| PAC-MAN | AAAAA | 🟡 Normal-path Stage 1 + pause/resume + one collision/life cycle + headed WAV sink; repeated lives and full play open | [→](AAAAA_pacman.md) |
| Mahjong | 77777 | 🟡 Readable title art + measured RLB stream under opt-in gates; gameplay open | [→](77777_mahjong.md) |
| Mini Golf | 88888 | 🟡 Loading/progress only | [→](88888_minigolf.md) |
| The Sims Bowling | 1500C | 🟡 Coherent title screen; follow-up/gameplay open | [→](1500C_simsbowling.md) |
| The Sims Pool | 1500E | 🟡 Coherent title screen; follow-up/gameplay open | [→](1500E_simspool.md) |
| Sudoku | 50513 | 🟡 Populated puzzle board/input partial under opt-in gates | [→](50513_sudoku.md) |
| Royal Solitaire | 50514 | 🟡 Coherent splash; RLB path diagnostic; readiness contract unresolved | [→](50514_royal_solitaire.md) |
| Bejeweled | 55555 | 🟡 PopCap Classic + two Action matches/timer | [→](55555_bejeweled.md) |
| Zuma | 44444 | 🟡 Live board + directional aim response; full play open | [→](44444_zuma.md) |
| Vortex | 12345 | 🟡 Full-color title/name entry + first-level HLE/SFX; pause-save freeze parity; gameplay/pause UI open | [probe](../game_tests/20260829_vortex_copy_lifecycle.md) |
| iQuiz | 11002 | ❌ Pack discovery/content loading blocked | [→](11002_twa.md) |
| SAT Prep Reading | 11050 | 🟡 Coherent splash/spinner; content handoff open | [SAT checkpoint](../game_tests/20260827_sat_loader_and_tetris_audio.md) |
| SAT Prep Writing | 11051 | 🟡 Coherent splash/spinner; content handoff open | [SAT checkpoint](../game_tests/20260827_sat_loader_and_tetris_audio.md) |
| SAT Prep Mathematics | 11052 | 🟡 Coherent splash/spinner; content handoff open | [SAT checkpoint](../game_tests/20260827_sat_loader_and_tetris_audio.md) |
| LOST | 1B200 | 🟡 Direct-HLE partial render; async asset path and first scenes reached; mode-7/input/audio parity open | [→](1B200_lost.md) |
| musika | 1C300 | 🟡 Splash only | [matrix](../game_tests/20260826_interactive_matrix.md#current-matrix) |

**Summary:** All 20 decrypted bundles launch far enough for a controlled
interactive probe, but none is yet certified fully playable. Tetris remains the
strongest puzzle result; Hold'em now reaches a coherent table on its normal
path, while Ms. PAC-MAN and PAC-MAN have the strongest arcade results, reaching
controllable Stage 1 mazes on their normal paths. The remaining titles need
content-specific input and renderer/asset fixes. The interactive matrix is the
current default-contract status source.

Latest interactive reports: `/tmp/fliwheel_interactive_full_{a,b,c,d}/interactive_matrix.md`.

## Running Games

### Launchers

These launch scripts exercise the rendering HLE and startup path; “working” in
older notes does not mean input, persistence, gameplay, and sound are all
verified.

```bash
./scripts/games/tetris.sh                # most tested
./scripts/games/cubis2.sh                # highest draw count
./scripts/games/holdem.sh                # complex poker game
./scripts/games/mspacman.sh              # classic arcade
./scripts/games/pacman.sh                # classic arcade
./scripts/games/mahjong.sh               # tile matching
./scripts/games/minigolf.sh              # golf game
./scripts/games/simsbowling.sh           # bowling sim
./scripts/games/simspool.sh              # pool sim
./scripts/games/iquiz.sh                 # pack/content loading
./scripts/games/sat-reading.sh           # SAT Prep Reading
./scripts/games/sat-writing.sh           # SAT Prep Writing
./scripts/games/sat-math.sh              # SAT Prep Mathematics
./scripts/games/musika.sh                # splash/runtime probe
./scripts/games/lost.sh                  # render-server probe
```

### PopCap / legacy partial-render Games

PopCap engine games still need a content-level renderer regression:

```bash
./scripts/games/bejeweled.sh             # startup/partial renderer probe
./scripts/games/zuma.sh                  # startup/partial renderer probe
```

The current DMA evidence and shared next gate are recorded in the
[PopCap DMA contract probe](../game_tests/20260826_popcap_dma_contract.md).

### Sudoku / Solitaire

Sudoku and Royal Solitaire use normalized coordinates and run directly:

```bash
./scripts/games/sudoku.sh /path/to/Games_RO/50513 --headless
./scripts/games/solitaire.sh /path/to/Games_RO/50514 --headless
```

### Common Script Options

```bash
./scripts/games/<game>.sh --timeout 15    # auto-terminate after 15s
./scripts/games/<game>.sh --headless      # no window (CI / testing)
./scripts/games/<game>.sh --verbose       # debug-level logging
./scripts/games/<game>.sh --dump 30       # dump first 30 frames as PPM
./scripts/games/<game>.sh --no-build      # skip cargo build
./scripts/games/<game>.sh --no-capture    # skip PPM frame captures
```

### Required Environment

All games require the experimental GL HLE renderer:

```bash
export FLIWHEEL_EXPERIMENTAL_GL_HLE=1
export FLIWHEEL_GL_GATE_B=1
export FLIWHEEL_GL_LIVE_CONTINUOUS=1
```

Presentation orientation is selected by the title-aware default where the
guest screen origin is known. Set `FLIWHEEL_GL_PRESENT_VFLIP=0|1` explicitly for
an orientation A/B experiment. The normalized-coordinate engines (Sims
Bowling/Pool, Sudoku, and Solitaire) and PopCap titles have title-specific
defaults.

### Bundle Directory

The current decrypted corpus used for regression is stored on the external
volume at:
```
/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/<bundle_id>/
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
| PopCap Engine | Zuma, Bejeweled | Pixel | Title default | ordinal-158 | **DMA** + .ipd |
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

### Solitaire — coherent splash, board open
- The shared NDC projection now preserves the character/UI quad positions and
  removes the previous full-screen slab corruption.
- The title still needs a state transition and card/selection interaction
  regression before calling the board correct.

### Vortex — direct-HLE title parity / gameplay open
- The PR #3 direct decrypted-eApp runner and fliwheel now agree on the early
  context/allocator contract and the late `circuits`, `bgAlpha`, `lava`, and
  `circuits_Door1` asset sequence.
- Generic GL color-array interpolation restores the colored ring and full title
  composition; no firmware boot is involved in this comparison.
- Select now drives the title into the `ENTER NAME` scene through the measured
  Vortex flags word; the post-transition scene is stable at 46 draws per frame.
- Need: map name entry and confirmation, reach the first content scene, then
  compare controls, sound, and long-run behavior.

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

### 2026-08-27: Sims normalized-coordinate casing fix
- Runtime bundle IDs are normalized to lowercase before the shared GL HLE is
  created
- The Sims NDC matcher now accepts the actual lowercase IDs (`1500c`/`1500e`)
- Both Sims titles render coherent title screens; menu/gameplay behavior remains
  unverified
- Evidence: [Sims NDC casing-fix probe](../game_tests/20260827_sims_ndc_casing_fix.md)

### 2026-08-27: Sims rectangle-target paletted upload
- OpenGLES:19 now accepts the Sims `GL_TEXTURE_RECTANGLE` target (`0x84f5`)
  for `GL_PALETTE8_RGBA8_OES` uploads
- Bowling decodes its `354x25` text atlas in the default live-GL path; Pool's
  ordinary `297x75` atlas path remains stable
- Both titles now have a small post-title text draw, but neither reaches menu
  or gameplay
- Evidence: [Sims rectangle-target texture probe](../game_tests/20260827_sims_paletted_texture_target.md)

### 2026-08-27: Sims RLB resource stream
- A pointer-valued preload result now lets an opt-in diagnostic path reach the
  guest's real `gameLib.rlb` parser
- Bowling and Pool both issue title-specific seek/read requests for multiple
  resource entries; neither has reached its menu or gameplay scene
- Evidence: [Sims RLB stream probe](../game_tests/20260827_sims_rlb_stream_probe.md)

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
- [2026-08-26 normalized-coordinate projection](../game_tests/20260826_ndc_projection.md)
- [2026-08-27 Sims rectangle-target texture](../game_tests/20260827_sims_paletted_texture_target.md)
- [Compatibility Report](../archive/reports/20260625_compatibility_report.md) — historical metrics
- [Debug Analysis](../archive/reports/debug_analysis.md) — historical root cause analysis
- [EAPP Format Specification](../EAPP_FORMAT_SPECIFICATION.md)
- [Legacy emulator architecture](../archive/firmware/EMULATOR_ARCHITECTURE.md)
