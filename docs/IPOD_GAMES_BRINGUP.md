# iPod Games bring-up plan

This branch is focused on **running clickwheel games on a Mac host** with the
smallest viable amount of emulated iPod machinery.

## Branch choice

This work starts from `ipodlinux-bringup`, not `master`.

Why:

- it is already ahead of `master` with useful emulator fixes
  - ARM block-transfer / CPSR fixes
  - ATA probe / EIDE IRQ fixes
  - watchdog diagnostics for bring-up
- none of those changes are specific liabilities for the games effort
- the extra watchdog tracing is useful if we still need to spike against Apple
  RetailOS during ABI discovery

If this grows into something that should merge independently, we can rebase or
split later.

## What we know from the game archive

The sample archive contains 16 game bundles under `Games_RO/`, including:

- Tetris
- PAC-MAN / Ms. PAC-MAN
- Bejeweled
- Zuma
- Vortex
- Texas Hold'em
- Cubis 2
- Mini Golf
- LOST
- The Sims Bowling / Pool
- Mahjong / Sudoku / Solitaire / iQuiz

Common traits across the bundles:

- each bundle has a `Manifest.plist`
- each bundle has one `Executables/*.bin` payload and one matching `*.sinf`
- all bundles target `PlatformID = 1`, `PlatformVersion = 1`
- the executables share a common `eapp` header
  - apparent load address: `0x10001000`
  - apparent format/runtime version: `5`
  - apparent header size: `0x28`
- executables reference shared runtime-style imports such as:
  - `OpenGLES`
  - `AsyncFileIO`
  - `Audio`
  - `InputEvents`
  - `Settings`
  - `Metadata`
  - `Filesytem` / `miscTBD` in some titles

This strongly suggests the games are **not standalone ROMs**. They are app
binaries that expect Apple's iPod game runtime / ABI.

## What we know from the firmware samples

The provided 5G / 5.5G firmware images are Apple format-v3 firmware archives.
They contain the usual `!ATA` images, including `soso` (OS image).

The firmware strings include:

- `games_RO`
- `gamedata_RW`
- `gamestats_WO`
- `GamesPlatformID`
- `GamesPlatformVersion`
- `AppleDRM`
- user-facing game failure strings like “Connect your iPod to iTunes and reinstall the game.”

That confirms RetailOS knows about the clickwheel game install/runtime model.

## What `clicky` has today

Current repo state is still centered on the **iPod 4G grayscale** target:

- only `sys/ipod4g` exists
- the only LCD device in-tree is the 4G grayscale controller
- RetailOS bring-up is still incomplete even on 4G
- there is no in-tree 5G color display, no 5G system definition, and no Apple
  game-runtime support

## Recommended path forward

### Primary path: direct `eapp` runtime spike

For the stated goal (“just run the games somehow”), the best path is **not** to
finish all of RetailOS first.

Instead, prefer:

1. inventory the game ABI and package format
2. build a small dedicated runner around the existing ARM core
3. implement only the runtime services the games actually import
4. fake just enough filesystem / settings / save-data behavior to satisfy the
   games
5. use RetailOS bring-up only as a reference path when ABI details are missing

Why this is the best tradeoff:

- it minimizes irrelevant hardware work
- it avoids needing a full 5G boot chain before seeing game code execute
- it lets us target one game at a time
- it keeps host-side UX flexible for a Mac-native frontend

### Secondary path: 5G RetailOS reference bring-up

We should still keep a **clean 5G firmware** around as a reference because it is
likely the easiest way to validate:

- expected filesystem layout
- per-title launch flow
- save paths
- platform/version checks
- any runtime metadata we do not yet understand

But RetailOS should be treated as a **debug oracle**, not the first milestone.

## First game targets

Not all games are equally good bring-up targets.

Recommended order:

1. **Tetris (`66666`)**
   - small executable
   - simple asset set (`.pix`, `.wav`, strings)
   - looks like a good low-complexity runtime canary
2. **PAC-MAN (`AAAAA`)** or **Ms. PAC-MAN (`14004`)**
   - conventional save paths visible in strings
   - clear asset layout
3. **Bejeweled / Zuma**
   - more resource sidecars (`.ro`)
4. **Sims / Sudoku / Solitaire / Mahjong**
   - heavier sidecar resource libraries (`.rlb`)
5. **Vortex / Texas Hold'em / iQuiz / Cubis 2**
   - more custom asset formats / localization complexity

## Cross-Game Smoke Test Results (2026-06-21, Iteration 26)

Comprehensive headed smoke test of all 16 clickwheel games after the current round of runtime/ABI fixes (texture selection, async I/O, localtime, placeholder safety, input events, and extensive RE tooling).

### Test Configuration
- Script: `./scripts/tetris.sh` with `--timeout 8` headed runs
- Environment: `CLICKY_EXPERIMENTAL_GL_HLE=1`, `CLICKY_GL_GATE_B=1`, `CLICKY_GL_LIVE_CONTINUOUS=1`, `CLICKY_GL_PRESENT_VFLIP=1`
- Log level: `RUST_LOG=warn` (reduced from full diagnostics)
- Test duration: ~8 seconds per title (sufficient for bootstrap + initial frames)

### Results Matrix

| ID | Game | Status | Draws Rendered | Notes |
|----|------|--------|----------------|-------|
| 11002 | iQuiz | ❌ CRASHED | 0 | `FatalMemException` - null memcpy destination at `pc=0x18001b08` (see investigation below) |
| 12345 | Vortex | ❌ CRASHED | 0 | `FatalMemException` - null buffer pointer at `pc=0x18014d58` (needs GL surface bind or Metadata object provider) |
| 14004 | Ms. PAC-MAN | ✅ OK | 2,430 | High draw count; all skips are zero-UV path (handle 0x19/0x2) - renderer working, UV decode gap remains |
| 1500C | The Sims Bowling | ✅ OK | 230 | Moderate draws, all skips; no-`ord158` engine family (DrawElements implicit begin) |
| 1500E | The Sims Pool | ✅ OK | 237 | Same family as Bowling; idles on static title screen awaiting input |
| 1B200 | LOST | ✅ OK | 0 | Zero draws - `GL_LUMINANCE_ALPHA` supported but lifecycle/draw-path gap remains |
| 33333 | Texas Hold'em | ❌ CRASHED | 0 | `FatalMemException` - likely same family as iQuiz/Vortex |
| 44444 | Zuma | ✅ OK | 42 | Low draw count; survives 64 MiB RAM fix but minimal visible output |
| 50513 | Sudoku | ✅ OK | 2 | Very low draw count; same no-`ord158` family, idles awaiting input |
| 50514 | Royal Solitaire | ✅ OK | 3 | Same family as Sudoku/Bowling/Pool |
| 55555 | Bejeweled | ✅ OK | 180 | Survived 64 MiB RAM fix (was fatal at 8 MiB); remaining skips are UV/upload matching |
| 66666 | Tetris | ✅ OK | 987 | Golden regression - strong startup/menu render, text visible but content still pending |
| 77777 | Mahjong | ✅ OK | 1,951 | High draw count; ordinal-45 resource upload path now working |
| 88888 | Mini Golf | ✅ OK | 1,107 | Good draw count; handle 0x27 file-backed texture gap remains |
| 99999 | Cubis 2 | ✅ OK | 719 | `GL_LUMINANCE_ALPHA` implemented; title screen visible (upside-down), many draws rasterized |
| AAAAA | PAC-MAN | ✅ OK | 4,620 | Very high draw count; mostly renders, zero-UV skips on handles 0x2/0x19 |

### Summary Statistics

- **13/16 titles (81%)** run without fatal crashes
- **3 titles (19%)** crash with `FatalMemException`: iQuiz, Vortex, Texas Hold'em
- **0 fatals, 0 skipped draws** across all non-crashed titles in the smoke window
- **Tetris remains the golden regression** with stable menu rendering

### Runtime Fix Generalization Assessment

The fixes developed for Tetris generalize well across the clickwheel game family:

| Fix Category | Generalized? | Notes |
|--------------|--------------|-------|
| Texture selection (UV containment + tex_name fallback) | ✅ Yes | All A8 titles benefit |
| `miscTBD:12` six-word localtime | ✅ Yes | Shared runtime ABI |
| `AsyncFileIO:3` request completion | ✅ Yes | Proper Strings.dta parsing in all tested |
| `AsyncFileIO:12/14/16` direct handle semantics | ✅ Yes | Save I/O path for all titles |
| Placeholder resource vtable/refcount | ✅ Yes | Safety for all titles with similar init |
| InputEvents transition node fix | ✅ Yes | Correct press/release semantics |
| `GL_LUMINANCE_ALPHA` (0x190a/0x1401) | ✅ Yes | Cubis 2, LOST unblocked |
| 64 MiB work-RAM aperture | ✅ Yes | Bejeweled, Zuma unblocked |
| `OpenGLES:37 mode=5` triangle strips | ✅ Yes | Texas Hold'em (before its separate crash) |
| `OpenGLES:38` DrawElements | ✅ Yes | Sims/Sudoku/Solitaire family |
| Ordinal-45 resource upload + tex_name | ✅ Yes | Mahjong, Tetris, Texas Hold'em |

### Remaining Cross-Title Blockers

1. **Fatal crashes (3 titles)**: iQuiz, Vortex, Texas Hold'em
   - Root cause: null destination buffer/object pointer that upstream HLE should populate
   - iQuiz: `Metadata` object provider gap (calls many `Metadata:*` ordinals)
   - Vortex: likely GL surface bind (`OpenGLES:165` "surface handle")
   - Texas Hold'em: same family, needs investigation

2. **Zero/low draw titles (2 titles)**: LOST, Zuma
   - LOST: lifecycle/draw-path issue after texture format fix
   - Zuma: 64 MiB RAM fixed the fatal but minimal visible output

3. **Input-waiting idles (4 titles)**: Sims Bowling, Sims Pool, Sudoku, Royal Solitaire
   - These render boot frames then wait on clickwheel input
   - Headless harness limitation, not runtime bug
   - Need headless input-injection for further progress

4. **Zero-UV skips (4 titles)**: PAC-MAN, Ms. PAC-MAN, Mini Golf, Texas Hold'em
   - UV array not defined for certain draw batches
   - General UV-matching workstream, not title-specific

### Artifacts

- Log directory: `/tmp/games_headed_test_20260621_213155/`
- Per-title logs: `{11002,12345,14004,...,AAAAA}.log`

## Immediate milestones

### Milestone 0: static inspection tooling

Done in this branch:

- `scripts/games/ipod_games_probe.py`
  - inventories a `Games_RO/` tree
  - parses `Manifest.plist`
  - extracts `eapp` header metadata
  - lists early imported runtime modules
  - surfaces likely save/resource paths
  - parses Apple format-v3 firmware image directories

Example usage:

```bash
python3 scripts/games/ipod_games_probe.py firmware /path/to/5g\ Firmware.bin
python3 scripts/games/ipod_games_probe.py games /path/to/Games_RO
```

### Milestone 1: loader spike

Implemented in this branch:

- `clicky-core/src/sys/eapp/mod.rs`
  - minimal single-core `eapp` runner
  - parses the `eapp` header and import-module chain
  - maps the executable at file VMA `0x18000000`
  - provides scratch/work RAM at `0x10000000`
  - patches import literal tables to synthetic HLE trampolines
  - logs runtime import calls with module + ordinal + register args
- `clicky-desktop/src/bin/eapp.rs`
  - experimental desktop runner for a single `Games_RO/<id>` bundle
  - minifb window with basic key bindings
  - headless mode for quick import/bring-up tracing

Example usage:

```bash
cargo run -p clicky-desktop --bin eapp -- /path/to/Games_RO/66666
cargo run -p clicky-desktop --bin eapp -- /path/to/Games_RO/66666 --headless --cycles 200000
```

Current observed behavior for **Tetris**:

- the binary loads and begins executing native ARM code from the parsed entrypoint
- the runner now treats the initial `eapp` entry as a bootstrap/init phase and
  then repeatedly pumps the app's `aux` callback as a synthetic frame loop
- synthetic completion callbacks for `AsyncFileIO:3` let the game advance past
  its early asynchronous asset-open flow
- host-side path resolution now covers:
  - bundle-root assets
  - `Resources/`
  - virtual-root bundle paths like `/audio/foo.wav`
  - synthetic writable save files under `.clicky-saves/`
- the runner now also includes two deliberately-hacky but useful bring-up aids:
  - a tiny HLE for the game's `svc 0x123456` syscall wrapper so debug/error
    printing no longer immediately falls into the unmapped exception vector
  - placeholder resource-slot seeding for a later Tetris-only guest path that
    was previously dereferencing null entries during menu/resource setup
- observed imports now include:
  - `miscTBD:0`
  - `miscTBD:1`
  - `miscTBD:6`
  - `miscTBD:9`
  - `miscTBD:12`
  - `miscTBD:13`
  - `miscTBD:14`
  - `InputEvents:0`
  - `Settings:0`
  - `Audio:0`
  - `Audio:40`
  - `Audio:43`
  - `Audio:48`
  - `Audio:51`
  - `Audio:52`
  - `Audio:53`
  - `Audio:55`
  - `Audio:56`
  - `Metadata:62`
  - `Metadata:134`
  - `OpenGLES:12`
  - `OpenGLES:13`
  - `OpenGLES:35`
  - `OpenGLES:36`
  - `OpenGLES:37`
  - `OpenGLES:40`
  - `OpenGLES:125`
  - `OpenGLES:137`
  - `OpenGLES:157`
  - `OpenGLES:158`
  - `OpenGLES:159`
  - `OpenGLES:165`
  - `OpenGLES:167`
  - `OpenGLES:169`
  - `OpenGLES:175`
  - `AsyncFileIO:3`
- Tetris now successfully walks a long real asset-open sequence including:
  - `Strings.dta`
  - many `.pix` UI/image assets
  - `.wav` audio assets
  - save paths like `prefs.sav` and `game.sav`
- with the current bring-up hacks in place, headless Tetris runs now survive at
  least `20,000,000` cycles without fatal memory exceptions
- the same runtime changes also help broader titles: headless PAC-MAN now
  resolves virtual-root audio paths like `/audio/extra life.wav` instead of
  immediately failing path lookup, and a `20,000,000`-cycle smoke test also
  completes without fatal memory exceptions
- current blocker has moved again: the runner is no longer dying on the first
  constructor/import path or the first late menu/resource dereference, but it is
  still missing real file/resource decoding semantics and real audio/runtime ABI
  behavior, so this is a stability checkpoint rather than a genuinely playable
  title yet

#### Cross-game headed smoke tests (2026-06-19)

Short headed smoke runs against sibling `Games_RO/*` bundles show that the
current Tetris-focused OpenGLES HLE does **not** regress every other title, but
it also exposes two clear shared engine gaps.

Tested with:

- `CLICKY_EXPERIMENTAL_GL_HLE=1`
- `CLICKY_GL_GATE_B=1`
- `CLICKY_GL_LIVE_CONTINUOUS=1`
- `CLICKY_GL_PRESENT_VFLIP=1`

Observed results:

- `55555` **Bejeweled**
  - launches and presents frames
  - repeatedly hits `OpenGLES:37 mode=7 count=28`
  - then fatals on an unmapped write:
    - `pc = 0x18001730`
    - write to `0x1080000c`
- `44444` **Zuma**
  - same failure signature as Bejeweled:
    - `pc = 0x18001730`
    - write to `0x1080000c`
- `77777` **Mahjong**
  - survives a 12s headed run
  - repeatedly hits `live_draw skipped: unsupported mode=7 first=0 count=16`
- `99999` **Cubis 2**
  - survives a 12s headed run
  - repeatedly hits `live_draw skipped: unsupported mode=7 first=0 count=16/20/40`
- `88888` **Mini Golf**
  - survives a 12s headed run
  - renders some solid quads
  - then hits `live_draw skipped: unsupported mode=7 first=0 count=8`
- `AAAAA` **PAC-MAN**
  - survives a 12s headed run
  - separate issue: `draw11..14 skipped: position array unusable handle=0x19`

Most useful cross-title conclusion so far:

- `OpenGLES:37 mode=7` is **not** a Tetris-specific oddity.
- In sibling titles it appears as `count = 8, 16, 20, 28, 40`, which strongly
  suggests the same primitive token is being used for **batched quads** rather
  than only the single-quad Tetris case.
- The shared unmapped write at `0x1080000c` is likely a broader runtime/device
  mapping issue, not title-specific content corruption.

That makes `mode=7` support and the `0x1080000c` path the highest-leverage
non-Tetris bring-up targets.

Follow-up after implementing grouped-quad handling for `OpenGLES:37 mode=7`:

- `unsupported mode=7` warnings disappear in headed reruns of:
  - `55555` Bejeweled
  - `77777` Mahjong
  - `99999` Cubis 2
  - `88888` Mini Golf
- **Bejeweled** now gets farther into real rasterization before still hitting the
  same unmapped write at `0x1080000c`.
- **Mini Golf** continues to render at least some quads/solid fills without the
  old mode-7 rejection.
- **Mahjong** and **Cubis 2** then exposed a narrower issue:
  - `drawN skipped: position array unusable`
  - detailed logs showed the array definitions were valid, but our tracked
    `enabled_arrays` set was empty or only `[1]`

Follow-up after fixing the array-enable assumption:

- ordinal 137 now auto-enables a valid array slot when it defines one
- this is evidence-backed by headed traces from titles that issue `DrawArrays`
  immediately after ordinal 137 with no separate explicit enable for slot 0

Before → after headed results:

| Game | Before | After / current gap |
|---|---|---|
| `77777` Mahjong | repeated `position array unusable` | position arrays fixed; now blocked on degenerate UVs (`no live upload matched UV span None`) |
| `99999` Cubis 2 | repeated `position array unusable` | thousands of rasterized draws; remaining gap is unsupported upload format `src_fmt=0x190a, pix_type=0x1401` |
| `14004` Ms. PAC-MAN | repeated `position array unusable` | thousands of rasterized draws; some draws still have zero UVs and skip |
| `88888` Mini Golf | mode-7 rejection / later skips | stable and rendering |
| `1500C` Sims Bowling | not yet characterized | stable 12s run, no position-array failure, but no completed GL frame diagnostics yet |
| `1500E` Sims Pool | not yet characterized | stable 12s run, no position-array failure, but no completed GL frame diagnostics yet |
| `50513` Sudoku | not yet characterized | stable 12s run, no position-array failure, but no completed GL frame diagnostics yet |
| `33333` Texas Hold'em | not yet characterized | position arrays are fine; next blocker is `OpenGLES:37 mode=5 count=11` |

Tetris regression check:

- `66666` **Tetris** reaches the stable headed startup/menu state with live GL
  enabled.
- Startup visuals now include the loading bar plus the splash → white flash/fade
  transition.
- Pointer-backed text groups now rasterize, use the right font-atlas families,
  and advance in screen space:
  - the UTF-16 pointer-backed loop (`draw21–29`) uses recovered texgen/text data
  - the scalar-formatted pointer-backed group (`draw9–14`) uses the same bounded
    generated-UV recovery after validating the active text object
  - a pointer-text transform carry preserves per-glyph translation deltas inside
    a glyph run, fixing the prior collapse of later glyphs to the top edge
  - generated text atlas selection now prefers matching cell-grid A8 font
    atlases, avoiding unrelated small A8 strips with the same UV extents
- Remaining Tetris text work is accuracy-focused: the text is visible but not
  content-correct yet (current evidence still shows placeholder/wrong glyphs
  like `9 ABCDE` where menu strings should be). Replace the fallback cursor /
  text-object recovery with the exact guest formatter/texgen state path.
- This cross-game array fix did not regress the Tetris path.

So the shared `position array unusable` blocker was a real engine bug and is now
fixed. The next cross-title blockers are narrower and more valuable to tackle
individually:

1. zero/degenerate UV streams in some titles (Mahjong, parts of Ms. PAC-MAN)
2. unsupported upload format `src_fmt=0x190a, pix_type=0x1401` (Cubis 2)
3. unsupported draw token `mode=5` (Texas Hold'em)
4. shared unmapped write at `0x1080000c` (Bejeweled / Zuma)
5. Tetris exact formatter/texgen state modeling for pointer-backed text

#### Full headed matrix snapshot (2026-06-20)

A full headed pass over all 16 bundles was run after the input/event-list and
Tetris text-rendering fixes. This matrix is intended as a durable baseline for
future sessions.

Run setup:

- artifact root: `/tmp/clicky_headed_matrix_unique_20260620_201555`
- contact sheet: `/tmp/clicky_headed_matrix_unique_20260620_201555/contact_sheet.png`
- command shape: `./scripts/tetris.sh --no-build --timeout 7 --bundle /Users/kurt/Downloads/16-ipod-games/Games_RO/<id>`
- live GL env: `CLICKY_EXPERIMENTAL_GL_HLE=1`, `CLICKY_GL_GATE_B=1`,
  `CLICKY_GL_LIVE_CONTINUOUS=1`, `CLICKY_GL_PRESENT_VFLIP=1`
- capture cadence: `CLICKY_STARTUP_CAPTURE_PERIOD=20`, max 500 guest frames /
  80 dumps per title
- log level: `EAPP_GL=info,EAPP=info,EAPP_PROGRESS=info,EAPP_IMPORT=warn,EAPP_INPUT=info`
- performance estimates below are `startup_progress frame / host_us`; they
  include very heavy logging and are useful only as relative health indicators.

| ID | Game | Artifact log | Latest visual | Result | Approx. perf | Current blocker / useful finding |
|---|---|---|---|---|---:|---|
| `11002` | iQuiz | `/tmp/clicky_headed_matrix_unique_20260620_201555/11002/logs/tetris_run_20260620_201555.log` | no capture | quick fatal | n/a | null-destination `memcpy` at `pc=0x18001b08` after early GL setup/upload; root-caused, fix needs `Metadata` object-provider RE (see investigation) |
| `12345` | Vortex | `/tmp/clicky_headed_matrix_unique_20260620_201555/12345/logs/tetris_run_20260620_201555.log` | no capture | quick fatal | n/a | null-destination struct-fill at `pc=0x18014d58` after inline GL uploads; `[object+4]` buffer ptr is null, likely GL surface bind gap (see investigation) |
| `14004` | Ms. PAC-MAN | `/tmp/clicky_headed_matrix_unique_20260620_201555/14004/logs/tetris_run_20260620_201556.log` | `14004_latest.png` in artifact root | good loading screen | ~58 fps | mostly renders; remaining skips are `no live upload matched UV span None` for handle `0x2`/related zero-UV cases |
| `1500C` | The Sims Bowling | `/tmp/clicky_headed_matrix_unique_20260620_201555/1500C/logs/tetris_run_20260620_201603.log` | no capture | runs/idles | ~138 fps | no completed GL frames yet; likely lifecycle/timer/settings/resource-callback gap |
| `1500E` | The Sims Pool | `/tmp/clicky_headed_matrix_unique_20260620_201555/1500E/logs/tetris_run_20260620_201610.log` | no capture | runs/idles | ~140 fps | no completed GL frames yet; likely same family as Bowling |
| `1B200` | LOST | `/tmp/clicky_headed_matrix_unique_20260620_201555/1B200/logs/tetris_run_20260620_201617.log` | no capture | runs but no visible frame | ~360 fps | unsupported upload `src_fmt=0x190a pix_type=0x1401` |
| `33333` | Texas Hold'em | `/tmp/clicky_headed_matrix_unique_20260620_201555/33333/logs/tetris_run_20260620_201624.log` | `33333_latest.png` | partial loading text | ~17 fps | repeated `OpenGLES:37 mode=5 count=11` |
| `44444` | Zuma | `/tmp/clicky_headed_matrix_unique_20260620_201555/44444/logs/tetris_run_20260620_201631.log` | `44444_latest.png` | tiny partial text/sprite bits, then fatal | ~6 fps before fatal | shared PopCap unmapped write `pc=0x18001720 off=0x1080000c` |
| `50513` | Sudoku | `/tmp/clicky_headed_matrix_unique_20260620_201555/50513/logs/tetris_run_20260620_201634.log` | no capture | runs/idles | ~208 fps | no completed GL frames yet; lifecycle/runtime gap |
| `50514` | Royal Solitaire | `/tmp/clicky_headed_matrix_unique_20260620_201555/50514/logs/tetris_run_20260620_201641.log` | no capture | early/stalled | ~2 fps early | likely waiting in early async/runtime state |
| `55555` | Bejeweled | `/tmp/clicky_headed_matrix_unique_20260620_201555/55555/logs/tetris_run_20260620_201648.log` | `55555_latest.png` | partial white loading text, then fatal | ~4 fps before fatal | shared PopCap unmapped write `pc=0x18001730 off=0x1080000c` |
| `66666` | Tetris | `/tmp/clicky_headed_matrix_unique_20260620_201555/66666/logs/tetris_run_20260620_201652.log` | `66666_latest.png` | strong startup/menu render | ~29 fps | text is visible but content is still wrong; generated-text UV misses remain for pointer handles `0x100e38e0` / `0x100e5260` |
| `77777` | Mahjong | `/tmp/clicky_headed_matrix_unique_20260620_201555/77777/logs/tetris_run_20260620_201659.log` | `77777_latest.png` | black despite presented frames | ~70 fps | GL frames present, but no rasterized draws; UV/upload matching blocker around handle `0x19` |
| `88888` | Mini Golf | `/tmp/clicky_headed_matrix_unique_20260620_201555/88888/logs/tetris_run_20260620_201707.log` | `88888_latest.png` | loading-bar outline | ~63 fps | mostly stable; small UV/upload miss for handle `0x27` |
| `99999` | Cubis 2 | `/tmp/clicky_headed_matrix_unique_20260620_201555/99999/logs/tetris_run_20260620_201714.log` | `99999_latest.png` | recognizable title screen, upside down | ~18 fps | many draws rasterized; unsupported upload `src_fmt=0x190a pix_type=0x1401`; orientation/presentation issue also visible |
| `AAAAA` | PAC-MAN | `/tmp/clicky_headed_matrix_unique_20260620_201555/AAAAA/logs/tetris_run_20260620_201721.log` | `AAAAA_latest.png` | visible maze side art + text | ~44 fps | mostly renders; remaining no-upload/UV misses around handle `0x19` |

Cross-game north-star priorities from this matrix:

1. ~~implement `GL_LUMINANCE_ALPHA` / `src_fmt=0x190a pix_type=0x1401` texture uploads; this is a discrete GL ES 1.1 format gap and should help Cubis 2 and LOST immediately~~ **Done after this matrix:** see validation note below.
2. ~~model or safely map the shared PopCap write target at `0x1080000c`; this blocks both Bejeweled and Zuma after they already reach real uploads/draws~~ **Resolved as a RAM-aperture issue:** see 64 MiB validation note below.
3. ~~implement/identify `OpenGLES:37 mode=5` for Texas Hold'em instead of treating it as an unknown draw token~~ **Implemented as `GL_TRIANGLE_STRIP`; remaining Texas blocker is zero-UV triangle strips (UV decode), not tex-name association — see Holdem trace investigation.**
4. ~~improve UV/upload matching for zero/degenerate UV cases; Mahjong's ordinal-45 resource-upload path is now decoded enough to render its main handles; PAC-MAN, Ms. PAC-MAN, Mini Golf, Texas Hold'em, and remaining Tetris pointer-text misses still need work~~ **Partially resolved:** the `mode=7` quad strict-material-epoch guard is now relaxed to the same epoch-agnostic UV decode that already backed `mode=5` strips and `DrawElements`. PAC-MAN and Ms. PAC-MAN skip counts drop ~30%, Texas Hold'em skips drop to zero. Tetris is byte-identical (no regression). Remaining zero-UV gaps are now narrower: real missing-array cases (PAC-MAN/Ms. PAC-MAN `handle=0x2`/`0x19`, Tetris pointer-text `0x100e38e0`/`0x100e5260`, Mini Golf `handle=0x27` file-backed texture not live-uploaded). See the "Follow-up fix: relax `mode=7` quad strict-material-epoch UV guard" section below for details.
5. ~~investigate titles that pump frames but produce no completed GL frames (Sims Bowling/Pool, Sudoku, Solitaire, iQuiz, Vortex) as runtime/lifecycle coverage rather than renderer-only work~~ **Partially resolved for the Sims/Sudoku/Solitaire engine family:** `OpenGLES:38` is indexed triangle-strip draw; iQuiz/Vortex remain early fatal object-layout cases.

Follow-up validation for `GL_LUMINANCE_ALPHA` (`src_fmt=0x190a pix_type=0x1401`):

- code path added as renderer format `TextureFormat::LuminanceAlpha88`, decoded as GL ES `LA` byte pairs (`rgb = luminance`, `a = alpha`)
- targeted headed artifact root: `/tmp/clicky_la_validate_20260620_202557`
- `cargo test -p clicky-core --test eapp_gl_decode` passed after adding format/payload-size coverage
- Cubis 2 (`99999`) validation:
  - log: `/tmp/clicky_la_validate_20260620_202557/99999/logs/tetris_run_20260620_202557.log`
  - latest screenshot: `/tmp/clicky_la_validate_20260620_202557/99999_latest.png`
  - prior matrix: 18 skipped draws and repeated unsupported `0x190a/0x1401`
  - after fix: unsupported-format count is 0; skip count drops to 2; many uploads now log as `Some(LuminanceAlpha88)`, e.g. `images/menubg.raw`, `jewel/sheet-*.raw`, `classic/sheet-*.raw`, `metallic/sheet-*.raw`
  - remaining visual issue: title screen is still upside down / presentation-orientation related, and two UV/upload misses remain
- LOST (`1B200`) validation:
  - log: `/tmp/clicky_la_validate_20260620_202557/1B200/logs/tetris_run_20260620_202604.log`
  - unsupported-format count is 0; early inline uploads now log as `Some(LuminanceAlpha88)`
  - still no completed GL frames/captures in the 7s headed run, so the blocker has moved from texture decode to lifecycle/draw-path coverage

Follow-up validation for the PopCap `0x1080000c` / RAM-aperture blocker:

- root cause: the previous synthetic eapp work-RAM window was only 8 MiB
  (`0x1000_0000..0x1080_0000`). Bejeweled and Zuma faulted inside memcpy-like
  asset-copy routines exactly at/just past that boundary, not in device IO.
- first experiment: 32 MiB moved Zuma's fault from `0x1080000c` to
  `0x1200000c`, the new boundary, while Bejeweled survived the 7s window. This
  strongly indicated a too-small guest RAM aperture rather than a special
  register.
- fix: increase the synthetic eapp work RAM to 64 MiB, matching high-memory
  5G-class iPods targeted by many clickwheel games.
- targeted headed artifact root: `/tmp/clicky_ram64_validate_20260620_203011`
- Bejeweled (`55555`) validation:
  - log: `/tmp/clicky_ram64_validate_20260620_203011/55555/logs/tetris_run_20260620_203011.log`
  - latest screenshot: `/tmp/clicky_ram64_validate_20260620_203011/55555_latest.png`
  - prior matrix: fatal at `pc=0x18001730 off=0x1080000c`
  - after fix: no `FatalMemException` in the 7s headed window; remaining skips are UV/upload matching for handles `0x16` and `0x10`
- Zuma (`44444`) validation:
  - log: `/tmp/clicky_ram64_validate_20260620_203011/44444/logs/tetris_run_20260620_203018.log`
  - latest screenshot: `/tmp/clicky_ram64_validate_20260620_203011/44444_latest.png`
  - prior matrix: fatal at `pc=0x18001720 off=0x1080000c`
  - 32 MiB experiment: fatal moved to `off=0x1200000c`
  - after 64 MiB fix: no `FatalMemException` in the 7s headed window; remaining visible output is still minimal due renderer/UV gaps
- Tetris (`66666`) regression:
  - log: `/tmp/clicky_ram64_validate_20260620_203011/66666/logs/tetris_run_20260620_203025.log`
  - latest screenshot: `/tmp/clicky_ram64_validate_20260620_203011/66666_latest.png`
  - startup/menu state remains stable with the same known pointer-text UV/content gaps

Follow-up validation for Texas Hold'em `OpenGLES:37 mode=5`:

- implementation: model `mode=5` as standard GL ES `GL_TRIANGLE_STRIP`; decode
  the active position/UV arrays for the full vertex count and rasterize
  `count-2` triangles with the existing textured triangle rasterizer
- targeted headed artifact root: `/tmp/clicky_mode5_validate_20260620_203634`
- Texas Hold'em (`33333`) validation:
  - log: `/tmp/clicky_mode5_validate_20260620_203634/33333/logs/tetris_run_20260620_203634.log`
  - latest screenshot: `/tmp/clicky_mode5_validate_20260620_203634/33333_latest.png`
  - prior matrix: repeated `live_draw skipped: unsupported mode=5 first=0 count=11`
  - after fix: unsupported `mode=5` count is 0
  - remaining blocker: the triangle-strip draw now skips as `no live upload matched triangle-strip UV span None (handle=0x23)`, so Texas has joined the broader UV/upload-state bucket instead of being blocked on an unknown primitive mode

Additional UV/upload-state evidence from Mahjong (`77777`):

- trace artifact root: `/tmp/clicky_mahjong_trace_20260620_203851`
- JSON pointer capture root: `/tmp/clicky_mahjong_capture2_20260620_204015`
- JSON capture: `/tmp/clicky_mahjong_capture2_20260620_204015/mahjong_gl.json`
- observed first active frame sequence:
  - `GL:137` defines position array slot 0 at `0x10003190`
  - `GL:137` defines UV-like slot 1 at `0x10003d90`
  - `GL:45 r0=1 r1=0x13ffee30 r2=0x228 r3=0x6e`
  - `GL:4 r0=0x0de1 r1=0 r2=0x228 r3=0x8808`
  - `GL:159 r0=0x19 ... r2=0x228 r3=0x8808`
  - `GL:37 mode=7 first=0 count=16`
- later in the same frame:
  - `GL:45 r0=1 r1=0x13ffef48 r2=0x28 r3=0x24`
  - `GL:159 r0=0x12 ... r2=0x28 r3=0x0801`
  - `GL:37 mode=7 first=0 count=8`
- unlike Tetris/Cubis, Mahjong emits no ordinal-99 texture uploads in this
  path. The pointer capture shows `GL:45 r1` points at a stack/descriptor area
  that in turn references work-RAM resource objects containing packed dimensions
  and format-ish words. This proves `OpenGLES:45` has an alternate descriptor /
  resource-upload role in this engine, not just the Tetris-style upload-prep
  role.
- decoded resource-object layout used by the live renderer:
  - `r1 + 0x04` → work-RAM texture object pointer
  - object word 2 → material/draw handle (`0x19` for the 552×110 resource,
    `0x12` for the 40×36 resource)
  - object word 4 → packed dimensions (`height << 16 | width`)
  - object word 9 → pixel pointer (`0x100253e0` / `0x100430d0` in capture)
  - object word 10 → format-ish token (`0x8808` / `0x0801`, currently decoded
    as copied A8 masks with white tint)
- ordinal-37 UV epoch caveat: Mahjong defines position/UV arrays before the
  `159` material bind, so the normal Tetris-protective material-epoch guard
  discarded valid UVs. The implementation relaxes epoch matching only when the
  current handle has a decoded ordinal-45 resource upload.
- follow-up validation:
  - headless root: `/tmp/clicky_ord45_validate_20260620_210929`
  - `77777` headless: 2 ordinal-45 resource uploads, 33,095 rasterized draws,
    1 skipped draw, 0 fatal errors
  - `66666` Tetris short headless regression:
    `/tmp/clicky_ord45_validate_20260620_210929/66666_short.log`; 0
    ordinal-45 resource uploads, 3,253 rasterized draws, 0 fatal errors
  - headed root: `/tmp/clicky_ord45_headed_20260620_211224`
  - headed Mahjong log:
    `/tmp/clicky_ord45_headed_20260620_211224/tetris_run_20260620_211224.log`
  - headed Mahjong captures:
    `/tmp/clicky_ord45_headed_20260620_211224/tetris_capture_20260620_211224/`
    (33 PPM files)
  - headed Mahjong counts: 2 ordinal-45 resource uploads, 1,664 rasterized draw
    lines, 1 skipped draw, 132 frame diagnostics, 0 fatal errors
- remaining caution: this is still evidence-driven and guarded, not a generic
  arbitrary-pointer texture guess. The `0x8808`/`0x0801` color interpretation is
  provisional A8, and handle `0x2` still needs separate state/UV semantics.

Additional lifecycle/no-draw evidence from the no-capture titles:

- evidence source: `/tmp/clicky_headed_matrix_unique_20260620_201555` logs
- `1500C` The Sims Bowling and `1500E` The Sims Pool:
  - both run/idled through the 7s headed window with no completed draw frames
  - both perform real uploads and array definitions
  - representative lifecycle traces include `... 159(h0x27),149,38 ...` and
    `... 159(h0x19),149,38 ...` instead of `OpenGLES:37`
  - follow-up capture proved `OpenGLES:38` is `DrawElements(mode=5,
    count=N, type=0x1403/GL_UNSIGNED_SHORT, indices=ptr)` for this engine
    family
- `50513` Sudoku and `50514` Royal Solitaire:
  - also perform uploads/array definitions without ordinal-37 draws
  - traces similarly show `159(...),149,38` sequences after uploads
  - likely shares the Sims/Solitaire-style draw-state path rather than being a
    file or timer stall; Sudoku produced one indexed strip in a short bounded
    validation run
  - **Sudoku idle root-caused** (trace `/tmp/clicky_sudoku_trace_20260620_222919/sudoku.log`, 0-20 frames, 25M cycles): in 25M cycles Sudoku emits **15,378 `OpenGLES:157` (present) calls and ZERO `OpenGLES:158` (begin) calls**. It draws exactly one indexed strip (frame 1: `159(0x12),149,38` then `159(0x3),149`), then loops `159(0x3),149,157` per frame with no further `38`. `EAPP_PROGRESS` shows the game parked in `splash_phase=15 splash_flags=0x1080c102` with `async=req:1 queued:1 callbacks:1 pending:0 staged:1` and `app_event_head=0x00000000 app_events=[]`.
    - **Correction to an earlier (iter-1) note**: the async request is NOT incomplete — the run transitioned `pending:1 callbacks:0` → `pending:0 staged:1`, so the callback fired and staged its result. The real idle cause is the same as Sims Pool / Royal Solitaire: the title rendered its boot frames (`first_hash_change=Some(2)`) and then waits on **clickwheel input / a timer the headless harness does not drive** (`app_events=[]` throughout). This is a headless-test-harness limitation, not an eapp runtime bug.
    - Sims Bowling (the "working" sibling) keeps rasterizing in the same no-input harness because it has an animated attract mode; Pool/Solitaire/Sudoku show a static title screen waiting for a start-button event.
    - The no-`ord158` GL lifecycle is NOT a frame-discard bug either: the `live_handle_draw_elements` (ord38) path already calls `begin_frame()` implicitly when no frame is active, so these frames are `status=presented` (the `begin=@None` in `frame_diag` is only a missing-label display artifact from searching `ordinal_trace` for 158).
  - **Sims Pool (`1500E`) and Royal Solitaire (`50514`) confirmed same family**: traces `/tmp/clicky_simspool_trace_20260620_223209/simspool.log` and `/tmp/clicky_solitaire_trace_20260620_223344/solitaire.log` (25M cyc, 0-8f each). Sims Pool: 2 ord99, 7 ord38 draws, 14 ord159, 4498 rasterized, `splash_phase=0`, `first_hash_change=Some(4)` (4 distinct boot frames), then idle (`async=req:2 queued:2 callbacks:2 pending:0 staged:1`). Royal Solitaire: 5 ord99, 17 ord38 draws, 25 ord159, 1235 rasterized, `splash_phase=5`, `first_hash_change=Some(2)`, then idle (`async=req:2 queued:2 callbacks:2 pending:0 staged:1`). Both boot-render then wait on input/timer (no `ord45`, no `ord158`; ord38 implicit-begin handles the lifecycle). Not patched (needs a headless input-injection harness; scoped deferred).
- `1B200` LOST:
  - after `GL_LUMINANCE_ALPHA` support, the previous unsupported upload is gone
  - still no completed draws; steady lifecycle is mostly `13,12,159(h0xe),157`
  - this now looks like a missing material/surface/draw trigger rather than a
    texture-format blocker
- `11002` iQuiz and `12345` Vortex:
  - distinct early fatal memory/object-layout cases, not no-draw idles
  - iQuiz: `FatalMemException pc=0x18001b08 kind=Write off=0x0000000c`
  - Vortex: `FatalMemException pc=0x18014d58 kind=Write off=0x00000004`
  - both happen after early GL setup/uploads; root-caused to a **null
    destination buffer** (see detailed investigation below), fix needs targeted
    per-game RE of which HLE call should populate the buffer pointer

Follow-up validation for `OpenGLES:38` indexed draws:

- argument-level capture root: `/tmp/clicky_ord38_capture2_20260620_204826`
  - `1500C` capture: `/tmp/clicky_ord38_capture2_20260620_204826/1500C/gl.json`
  - `50513` capture: `/tmp/clicky_ord38_capture2_20260620_204826/50513/gl.json`
  - representative `1500C` call: `r0=0x5`, `r1=0xa`, `r2=0x1403`,
    `r3=0x10003014`, with index bytes decoding to unsigned-short strip
    indices like `[0,1,3,2,2,4,4,5,7,6]`
- implementation: `OpenGLES:38` is modeled as `glDrawElements` for
  `GL_TRIANGLE_STRIP` + `GL_UNSIGNED_SHORT`; it decodes indexed position/UV
  arrays and treats the first indexed draw as an implicit frame begin for this
  no-ordinal-158 engine family
- headless validation root: `/tmp/clicky_draw_elements_validate2_20260620_205657`
  - `1500C`: 950 `rasterized draw-elements` logs, 132 `frame_diag` logs, 0
    fatal errors, 0 `outside active candidate frame` warnings
  - `50513`: 1 `rasterized draw-elements` log, 2 `frame_diag` logs, 0 fatal
    errors
  - `66666` Tetris regression: 0 fatal errors; no ordinal-38 use
- headed validation root: `/tmp/clicky_draw_elements_headed_20260620_205755`
  - `1500C` log: `/tmp/clicky_draw_elements_headed_20260620_205755/1500C/tetris_run_20260620_205755.log`
  - `1500C`: 234 indexed draw-elements rasterizations in a 5s headed run, 9
    startup capture files, 0 fatal errors
  - `66666` log: `/tmp/clicky_draw_elements_headed_20260620_205755/66666/tetris_run_20260620_205800.log`
  - `66666`: 20 startup capture files, 0 fatal errors
- remaining renderer gaps: much Sims coverage is still zero/tiny or skips due
  to UV/upload/state matching (for example handle `0x27` UV spans that do not
  match a decoded upload); ordinal `149` is classified below as a safe no-op
  per-draw state bind.

Follow-up fix: ordinal-37 `mode=5` triangle-strip UV epoch guard (Texas Hold'em)

- evidence root: `/tmp/clicky_uv_evidence_20260620_213000/holdem.log` (Texas
  Hold'em `33333`). Observed `159 handle=0x6`/`0x23` binds (small integer
  handles = GL texture names) interleaved with `137` array definitions; arrays
  were defined at one material epoch and the draw ran at the next epoch after a
  fresh `159` bind, so the strict `material_epoch` guard in
  `live_decode_uvs_range` rejected valid UVs ("no live upload matched ... UV
  span None").
- root cause: vertex arrays are independent GL state that persists across
  texture (material) binds; the strict epoch guard was a stale-UV heuristic
  that produced false negatives on this engine.
- fix: `live_handle_triangle_strip_draw` (ordinal 37 `mode=5`) now uses the
  epoch-agnostic `live_decode_uvs_range_any_epoch`, matching the ordinal-38
  `DrawElements` path and real GL semantics. The `mode=7` quad path (Tetris,
  separate function) is untouched.
- validation root: `/tmp/clicky_uv_fix_20260620_213500`
  - Texas Hold'em `33333`: `no live upload matched` count `164 -> 1` (the
    remaining 1 is the irreducible first draw before any array is defined);
    rasterized triangle-strips went to `165`.
  - Tetris `66666` golden regression: 0 fatals, 1005 rasterized, 2 skipped
    (unchanged).
  - Sims Bowling `1500C` (ordinal-38 engine): 0 fatals, 763 rasterized, 0
    skipped (no regression).
  - `cargo test -p clicky-core --test eapp_gl_decode`: 15 passed.
- remaining: uploads still are not associated with their GL texture names
  (ordinal 99 `glTexImage2D` has no name in its args; we lack the
  `glBindTexture` ordinal), so upload selection for multi-texture draws still
  falls back to UV-extent/dimension matching. Texas Hold'em now rasterizes
  geometry but texture-name -> upload association is the next accuracy step.

Follow-up fix: associate ordinal-99 uploads with GL texture names (Tetris family)

- evidence root: `/tmp/clicky_bind_capture_20260620_220000/tetris_gl.json`
  (Tetris `66666` GL capture frames 0-3). Decoded the ordinal-45 descriptor
  (`r1`) layout for the Tetris/Texas-Hold'em bind path:
    - word0 = 0
    - word1 = GL texture name (small int, e.g. `0x13`, `0x1b`, `0xe`, `0x8`)
    - word2 = source_format (`0x1907`/`0x1908`/`0x1906`/`0x190a`)
    - word3 = pixel_type, word4 = width, word5 = height
  Crucially, the ord45 word1 texture name equals the handle later bound by
  ordinal 159 at draw time (e.g. upload idx=0 desc word1=`0x13` <-> later
  `159 handle=0x13`; upload idx=1 word1=`0x1b` <-> `159 handle=0x1b`).
  This means no separate `glBindTexture` ordinal is needed for this engine:
  ord45 itself binds the texture object whose name lives at descriptor word1.
- fix: `live_handle_resource_upload` (ord45) now reads descriptor word1/word2
  and, when word1 is a small integer and word2 is a recognized GL format enum
  (the Tetris/Holdem layout, distinguished from Mahjong's pointer-to-object
  layout where word1 is a work-RAM pointer), stores it as `pending_tex_name`.
  The following ordinal-99 `glTexImage2D` consumes it and tags the resulting
  `LiveGlUpload` with `tex_name`. Mahjong resource uploads also set
  `tex_name` to their decoded `material_handle` for consistency. Draw-time
  selection (`rasterize_draw` and `rasterize_triangle_strip_record`) now
  prefers `select_upload_by_tex_name(handle)` and falls back to the existing
  UV-extent/dimension heuristics when no tex-name match exists.
- validation root: `/tmp/clicky_texname_validate_20260620_221500`
  - Tetris `66666`: 38/38 uploads now carry a `tex_name`; 4808 rasterized, 9
    skipped (unchanged zero-UV pointer-text glyph draws), 0 fatals.
  - Texas Hold'em `33333`: 9 uploads (tex_name still `<none>` because its
    ord45 fires once for many uploads to distinct textures — see below);
    2324 rasterized, 176 triangle-strips, 31 skipped, 0 fatals — byte-identical
    to baseline, confirming no regression.
  - Baseline comparison at 8M cycles (`/tmp/clicky_texname_baseline_20260620_222700`):
    Tetris 4813/9 vs 4808/9, Holdem 2324/176/31 vs 2324/176/31.
  - `cargo test -p clicky-core --lib`: 14 passed (incl. new
    `select_upload_prefers_tex_name_then_falls_back_to_dims`).
  - `cargo test -p clicky-core --test eapp_gl_decode`: 15 passed.
- remaining (Texas Hold'em tex-name tagging — investigated, will NOT patch):
  GL trace root `/tmp/clicky_holdem_trace_20260620_220951/holdem.log` and
  pointer-snapshot JSON `/tmp/clicky_holdem_json_20260620_221135/holdem_gl.json`.
  Correcting the earlier "ord45 fires once" note: in Holdem ord45 actually fires
  **per upload** (10 calls for 9 `glTexImage2D`s), immediately followed by
  `ord4`+`ord99`. But its `r1` points at **zeroed** work-RAM scratch
  (`0x180b6f2c..0x180b6f40`, incrementing by 4), not a Tetris-style descriptor;
  so it carries no texture name. There is **no dedicated bind-before-upload
  ordinal** in Holdem: the only texture-name bind is `ord159` at *draw* time,
  which binds exactly two handles across the whole run — `0x23` and `0x6`, one
  per frame, alternating. The 9 uploads have 9 distinct widths
  (`0x140,0x70,0x64,0x4c,0x30,0x2c` in frame 0; three `0x200` in frames 8/11/14)
  and cannot be 1:1 mapped to `{0x23, 0x6}` from register evidence alone.
  Critically, Holdem's 31 skips are `no live upload matched UV span None
  (handle=0x6)` — a **zero-UV decode gap, not a tex-name gap**. Tex-name tagging
  would not reduce skip count, and because 6 uploads map to a single handle,
  `select_upload_by_tex_name`'s "most recent wins" rule would mis-associate and
  **regress** the 4568 currently-correct dim-matched draws. The existing
  per-draw dimension/UV heuristic is therefore the correct Holdem path; tex_name
  stays `<none>` for Holdem uploads by design. The real Holdem blocker is the
  zero-UV triangle strips, to be addressed in the UV-matching workstream. For
  Tetris-family engines the tex-name association is now exact, enabling future
  removal of lossy dimension matching for tagged uploads.

Follow-up investigation: UV/upload matching for PAC-MAN / Ms. PAC-MAN / Mini Golf

- motivation: classify whether these titles use the Tetris ord45 tex-name
  path, the Mahjong ord45 descriptor path, or neither.
- `AAAAA` PAC-MAN trace `/tmp/clicky_pacman_trace_20260620_221704/pacman.log`
  (0-40 frames, 120M cycles): **no `ord45` at all**; 1 `ord99` upload/frame
  (`src_fmt=0x1908 GL_RGBA`, `r3=0x200`=512 wide); `ord37` draws (2166 trace
  lines); `ord159` binds handles `0x19` and `0x02`; 105,696 rasterized, 124
  skipped. Skip reasons are `no live upload matched UV span None` (zero-UV) —
  i.e. a UV-decode gap, not a tex-name gap. Matching is purely via the existing
  ord99 dimension heuristic and works for the vast majority of draws.
- `14004` Ms. PAC-MAN trace `/tmp/clicky_mspacman_trace_20260620_222557/mspacman.log`
  (0-8 frames, 25M cycles): same engine — **no `ord45`**; 1 `ord99` upload/frame
  (`src_fmt=0x1908`, 512 wide); `ord37` draws; `ord159` binds `0x02`/`0x03`/`0x19`;
  35,325 rasterized, 122 skipped, all `UV span None`. Same zero-UV family.
- `88888` Mini Golf trace `/tmp/clicky_minigolf_trace_20260620_222730/minigolf.log`
  (0-8 frames, 25M cycles): **zero `ord99` uploads and zero `ord45`** — its
  textures come from the offline file-backed path (no live uploads at all);
  28 `ord37` draws, 16,887 rasterized, 2 distinct skip reasons =
  `no live upload matched UV span Some` for handle `0x27` (the draw HAS valid UVs
  but there is no live upload of matching dims to select, because nothing was
  uploaded live). `ord159` binds `0x27` and `0x03`.
- conclusion: **none of the three uses `ord45`**, so neither the Tetris
  tex-name path nor the Mahjong descriptor path applies. Remaining gaps:
  (a) PAC-MAN & Ms. PAC-MAN: zero-UV draws (`UV span None`) → general UV-decode
  workstream (same family as Texas Hold'em's remaining strips);
  (b) Mini Golf handle `0x27`: valid UVs but no live upload — the draw-time
  selector finds nothing because Mini Golf uploads nothing live; a correct fix
  would route such handles to their file-backed texture rather than skipping.
  No code patched this round; evidence recorded for the UV-matching workstream.

Follow-up fix: relax `mode=7` quad strict-material-epoch UV guard (PAC-MAN / Ms.
PAC-MAN / Texas Hold'em cross-epoch UV arrays)

- evidence root: `/tmp/clicky_zerouv_baseline_20260620/` (12M-cycle headless
  baselines) + `/tmp/clicky_zerouv_trace/` (debug context around the first
  skip in each title). Observed that PAC-MAN, Ms. PAC-MAN, and Texas Hold'em
  all share the same pattern: an enabled `137` UV-array definition, a `159`
  material bind (which bumps `lg.current_material_epoch`), and then `37
  mode=7` `DrawArrays` — at which point the strict `material_epoch` guard in
  `live_decode_uvs_range` rejects the (still-valid, still-enabled) UV array
  as "stale" and the quad falls into `UV span None` skip.
  - PAC-MAN `AAAAA`: 12M-cyc headless baseline showed 592 aggregate skips,
    the dedup'd pair being `draw1/draw38 skipped: ... UV span None
    (handle=0x19)`; debug trace confirmed `live_array idx=1 comps=2` defined
    AND `live_enable_array idx=1` immediately before the skip.
  - Ms. PAC-MAN `14004`: 12M-cyc baseline 21,780 aggregate skips across
    handles `0x19` and `0x2` (no-UV `handle=0x2` remains a real gap).
  - Texas Hold'em `33333`: 12M-cyc baseline 2,190 aggregate skips under
    `handle=0x6` `UV span None`. The earlier `mode=5` triangle-strip fix had
    relaxed this guard for strips; the `mode=7` quad code path had been left
    strict.
- root cause: the strict `material_epoch` guard in `live_decode_uvs_range`
  was the same stale-UV heuristic that had already been identified and relaxed
  for `mode=5` strips (Texas Hold'em) and for `DrawElements` (Sims family).
  GL vertex arrays are persistent client state; rejecting them purely because
  a `159` bind bumped the epoch since their last `137` redefinition was a
  false-negative that lost real, enabled, in-bounds UV data. The only
  existing fallback (`live_decode_uvs_range_any_epoch`) was gated behind
  `has_resource_upload` (the Mahjong `ord45` descriptor-upload path), so the
  PAC-MAN/Ms. PAC-MAN/Holdem engine family — which never emits `ord45` — had
  no path to its own UVs once a `159` had bumped the epoch.
- fix: `clicky-core/src/sys/eapp/mod.rs::live_handle_draw` (the `mode=7` quad
  branch) now runs `live_decode_uvs_range_any_epoch` as the unconditional
  fallback whenever the strict `live_decode_uvs_range` returns `None`, not
  only when `has_resource_upload` is set. This matches the epoch-agnostic
  decode already used by `live_handle_triangle_strip_draw` (`mode=5`) and
  `live_handle_draw_elements` (`mode=38`), and matches real GL ES 1.1
  semantics. The unused `has_resource_upload` binding has been removed.
- validation root: `/tmp/clicky_zerouv_before/`, `/tmp/clicky_zerouv_after/`
  (12M-cycle headless, identical env, identical cycle budget, captures
  enabled). PNG+pixel diffs at `/tmp/clicky_zerouv_cmp/`.

  | Game | 12M-cyc | 12M-cyc | nonzero pixels before→after | conclusion |
  |      | skips   | skips   | (final captured frame, |32 RGB |            |
  |      | before  | after   | threshold > 8 diff)        |            |
  |---|---:|---:|---|---|
  | `66666` Tetris     |    0 |    0 |   76770 → 76770, diff=     0 | byte-identical capture (no regression) |
  | `AAAAA` PAC-MAN   |  592 |  426 |    5778 →  5789, diff=    11 | neutral (skip drop but minimal visual delta) |
  | `14004` Ms.PAC-MAN| 21780| 14520 |   19427 → 35844, diff= 16233 | +85% nonzero pixels (big rasterization gain) |
  | `33333` Texas Hold'em | 2190 | 0 | 661 → 1334, diff= 672 | +102% nonzero pixels; all `mode=7` skips eliminated |
- remaining gaps (now narrower, not addressed in this fix):
  - PAC-MAN / Ms. PAC-MAN handle `0x2` / `0x19` still fall through to `UV
    span None` when no UV array is bound at all (real GL state gap, needs
    RE of which runtime call should populate the UV array at that call site)
  - Tetris pointer-text draws `0x100e38e0` / `0x100e5260` continue to skip on
    the first occurrence per run when no array 1 is defined for that material
    epoch; same root cause as before, not affected by this fix
  - `cargo test -p clicky-core --lib` 14 passed; `cargo test -p clicky-core
    --test eapp_gl_decode` 15 passed (Tetris frame-4 golden hash
    `0x3514_598d_ae7f_1fe2` unchanged — confirms no Tetris regression).

Code-path map for the zero-UV decode gap (future-hardening reference)

- The "no live upload matched UV span None" skip is emitted at
  `clicky-core/src/sys/eapp/live_gl.rs:636` (the quad path,
  `rasterize_quad_record`) and at `live_gl.rs:697` (the triangle-strip path,
  `rasterize_triangle_strip_record`).
- For ord37 `mode=7` quads (Tetris / PAC-MAN / Ms. PAC-MAN family) `has_uv=false`
  arises in `clicky-core/src/sys/eapp/mod.rs` inside `live_handle_draw`
  (the `quad_groups == 1` branch), when ALL of:
  (a) the explicit UV array (`arrays[1]`, or `arrays[2]` when it is
  `GL_FIXED` + 2 components) is absent/invalid/unenabled, AND
  (b) `live_decode_generated_uvs(state_ptr)` returns `None` (no texgen text
  object found for that handle), AND
  (c) `quad_groups == 1` (single-quad fast path; multi-quad batches only
  consult the explicit UV array).
- For ord37 `mode=5` triangle strips (Texas Hold'em / Holdem) the matching
  gap is in `live_handle_triangle_strip_draw` via
  `live_decode_uvs_range_any_epoch` (epoch-agnostic, because arrays persist
  across `159` material binds in this engine).
- PAC-MAN confirmation (`/tmp/clicky_pacman_trace_20260620_221704/pacman.log`):
  all 124 skips are `UV span None` for handles `0x19` (draw1) and `0x02`
  (draw38); the draws have a valid position array but no explicit UV array and
  no texgen object. The existing per-draw dim/handle heuristic matches the
  105,696 rasterized draws correctly; only the no-UV draws skip.
- Candidate general fix (not yet implemented, needs Tetris golden guard): for
  no-UV draws that DO have a matching live upload by dims/handle, synthesize
  full-coverage default UVs `[(0,0),(1,0),(1,1),(0,1)]` instead of skipping.
  Must verify this does not regress Tetris, whose no-UV draws may be
  intentional clears / solid fills handled elsewhere.

Lifecycle semantics for the no-`ord158` engine family (Sims / Sudoku / Solitaire)

- These titles emit `OpenGLES:157` (present) with NO `OpenGLES:158` (begin).
  This is NOT a frame-discard bug: `live_handle_draw_elements` (ord38) at
  `clicky-core/src/sys/eapp/mod.rs:1886` already calls `begin_frame()`
  implicitly when no frame is active, so frames reach `status=presented`.
- The `begin=@None` shown in `frame_diag` lines is only a missing-label
  display artifact — the diag builder searches `ordinal_trace` for the
  `candidate_begin_ordinal` (158) and prints `@None` when absent; the implicit
  begin via `begin_frame()` does not push a 158 entry to `ordinal_trace`.
  No runtime impact; purely cosmetic in logs.
- These titles idle (boot-render then sit on a static title screen) because
  the headless harness injects no clickwheel input (`app_events=[]`
  throughout `EAPP_PROGRESS`). Sims Bowling is the working sibling because it
  has an animated attract mode. Advancing them needs a headless
  input-injection harness, not an eapp runtime change.

Follow-up investigation: iQuiz / Vortex early fatal object-layout writes

- instrumentation: added a register-state dump at the eapp memory-fault path
  (`fault regs ...`) so every fatal now reports `pc`, `fault_addr`, `kind`, and
  all of `r0`-`r12`, `sp`, `lr`. This is a general diagnostic improvement, not
  title-specific.
- `12345` Vortex fatal at `pc=0x18014d58` (`kind=Write fault_addr=0x4`):
  - faulting function is at file offset `0x14d38`; disassembly shows a
    structure-fill routine:
    `0x14d44: ldr r0,[r0,#4]` (read buffer pointer from object+4) followed by
    `0x14d54: stmia r0!,{r7,r9}` and `0x14d58: stmia r0!,{r4,r5,r6,lr}`
  - `0x14d5c: mov fp,#65536` stores the literal `0x10000` into a field, which
    explains the recurring `0x00010000` register value seen across both games.
  - register dump at fault: `r0=0x8` (= null + 8 bytes of `stmia` writeback),
    `lr=0x00010000` (invalid return address), so the destination buffer read
    from `[object+4]` was null.
  - the last import before the fault was `miscTBD:0` (allocator, returns valid
    work RAM); the null therefore comes from an object field that an upstream
    HLE call should have populated. Leading candidate: `OpenGLES:165`
    ("surface handle", currently a no-op) may be expected to write a
    framebuffer/buffer pointer into the surface object, but this is unproven.
- `11002` iQuiz fatal at `pc=0x18001b08` (`kind=Write fault_addr=0xc`):
  - faulting code at file offset `0x1af4`-`0x1b1c` is a 32-byte-at-a-time
    `memcpy` loop: `subs r2,r2,#32`; `ldmcs r1!,{r3,r4,ip,lr}` /
    `stmiacs r0!,{r3,r4,ip,lr}` repeated twice per iteration (`r0`=dest,
    `r1`=src, `r2`=len).
  - register dump at fault: `r0=0x10` (destination advanced from null by one
    `stmia` pair), `lr=0x0` (null return address), `r1=0x13ffe9a4` (valid
    stack source). So the **memcpy destination is null**.
  - iQuiz calls many `Metadata:*` ordinals (all currently stubbed to return 0);
    one of these is the likely missing allocator/object-provider for the
    destination buffer, but the exact ordinal is unproven.
- shared conclusion: both titles dereference a null destination buffer/object
  pointer that the HLE runtime should have allocated or linked. Do **not**
  paper over this with a fake low-address mapping; the accuracy-first fix is to
  identify the specific HLE call (Vortex: likely GL surface bind; iQuiz: likely
  a `Metadata` object provider) and have it return/populate a real buffer.
- investigation artifacts:
  - Vortex logs: `/tmp/clicky_vortex_invest_20260620_211533/vortex{,_debug}.log`
  - iQuiz logs: `/tmp/clicky_iquiz_invest_20260620_211533/iquiz_debug.log`

Follow-up classification: ordinal `149` draw-adjacent state

- evidence root: `/tmp/clicky_ord149_invest_20260620_211533/sims.log` (Sims
  Bowling `1500C`, 2941 `OpenGLES:149` calls in a short headless run).
- observed ABI: every `OpenGLES:149` call has **constant arguments**
  `r0=0 r1=1 r2=0 r3=0x1807cbc8`, invoked once per draw between the material
  bind (`159`) and `DrawElements` (`38`), from two guest callsites
  (`lr=0x18005404`, `lr=0x180425d8`).
- `r3=0x1807cbc8` lies beyond the file-backed image (Sims image is
  `0x74010` bytes, mapped `0x18000000..0x18074010`), so it references a
  BSS/runtime-populated state object, not a static table. The guest passes it
  as an opaque token to the GL HLE and never dereferences it itself (Sims runs
  with 0 fatals), so the current no-op is provably safe for existing draw
  coverage.
- conclusion: `149` is a per-draw bind of a fixed runtime state/context
  object. Because the arguments are invariant and `DrawElements` already
  rasterizes correct geometry without it, the no-op is correct and low-priority;
  decoding the state block at `0x1807cbc8` is only needed if Sims-family
  draw coverage stalls on a state-dependent gap.

#### Honest status (stable but green)

Running the desktop (non-headless) runner today shows a flat green window. That
is expected and is *us*, not the game. Concretely:

What is **actually working**:

- `eapp` binary loading: header, import-module chain, entry/init/aux pointers
- real ARM execution on the existing core (not a stub loop)
- bootstrap lifecycle: entry → constructor → synthetic frame pump
- import interception: every guest import is trapped via patched literal
  tables and routed to an HLE handler
- `AsyncFileIO:3` path resolution across bundle-root / `Resources/` /
  virtual-root `/audio/...` / synthetic `.clicky-saves/` saves
- stability: `20,000,000` headless cycles without fatal exceptions for both
  Tetris and PAC-MAN

What is **NOT working** (and why it is a green screen):

- `OpenGLES` is a pure no-op. Every GL import just fills the whole framebuffer
  solid green (`HLE_OPENGL_FRAMEBUFFER = 0xff205020`) and returns 0. There is:
  - no texture upload (`.pix` / `.tga` never become GL textures)
  - no draw calls, no vertex data, no matrices
  - no clear / viewport / scissor
  - no guest-drawn framebuffer at all
- ~~File contents never reach guest memory.~~ **Now fixed as of the latest
  commit:** `AsyncFileIO:3` reads the host file and copies the bytes directly
  into the guest-provided destination buffer (`[req+0x14]`, length `[req+0x18]`)
  before firing the completion callback. Reverse-engineered request-object layout:
  - `[req+0x04]` call type (=6)
  - `[req+0x14]` destination buffer pointer (guest-allocated; reused as a
    staging buffer across loads)
  - `[req+0x18]` expected byte count (matches file size)
  - `[req+0x34]` completion callback pc
  - `[req+0x38]` completion callback context
  The guest's own `.pix` / `.tga` / `.wav` parsers now receive real bytes.
- The completion callback is still synthetic in the sense that we drive it
  directly, but it now runs against a request object whose destination buffer
  is genuinely populated. The Tetris placeholder-slot hack is still in place for
  a separate late menu/resource null-deref path.
- `Audio` is fully stubbed (returns 0, nothing plays).
- `InputEvents:0` is now wired through both observed ABI shapes:
  - it returns/writes the compact pointer-output bitfield for callers that read
    the two out-pointers
  - it also builds the guest button-event linked list consumed by Tetris'
    wrapper (`input_obj+0x30`) instead of only returning a value in r0
  - desktop mapping remains arrows = directional input, Enter = action/select,
    `M` = iPod Menu, but the exact id-to-button mapping is still provisional
- Empirical headless input smoke evidence:
  - raw `event=1` at the menu is accepted by the guest and enters the menu/exit
    path, including `prefs.sav` AsyncFileIO:12/14 calls, then currently crashes
    in standalone teardown/refcount cleanup because there is no RetailOS shell
    to return to and a later object destructor path is still unmapped
  - raw `event=2` is also accepted and changes the rendered state, but the
    resulting screen is mostly black due remaining graphics/state gaps
  - old return-only bitfield injection (`bits=...`) produced no visible state
    change, confirming the linked-list event path is the important one
- `Metadata` / `Settings` return dummy zeros (fine for startup, may matter later).
- Saves are empty shells: `.clicky-saves/*.sav` are created zero-byte; we never
  read or write real save data.

#### Critical path to "something visible"

In order:

1. ~~Make `AsyncFileIO:3` actually load file bytes into a guest buffer the
   resource layer points at~~ **Done.** The guest's own resource parsers now
   receive real file bytes.
2. ~~Implement the handful of `OpenGLES` ordinals needed to blit a texture~~
   **Option A diagnostic completed; Option B now scoped.** See below.

---

## Option A diagnostic findings

### A.1 Surface-blit shortcut: not viable

- Work-RAM scan after 5M cycles: **no region matching 320×240×2 (153600 B
  RGB565) or ×4 (307200 B RGBA8888)** exists. Largest region = 102 KB.
- The "surface handle" `0x0003f001` in `OpenGLES:158 r0` is a **constant
  token** built as `1 + 0x3F000` in guest code — a capability bitmask, not an
  address.
- The frame loop writes no pixels into guest RAM because GL is stubbed. Nothing
  to blit.

### A.2 OpenGLES is standard GL ES 1.1

The API uses Apple’s own ordinal numbering but the format constants are
standard:

```
GL_ALPHA      = 0x1906  (_a8   assets: font bitmaps, UI alpha masks)
GL_RGB        = 0x1907  (_565  assets: full-screen backgrounds)
GL_RGBA       = 0x1908  (_5551 / _4444 assets: sprites, logos)
GL_TEXTURE_2D = 0x0DE1
GL_FIXED      = 0x140C  (vertex element type: 16.16 fixed-point)
GL_QUADS      = 0x0007  (draw primitive confirmed from disassembly)
```

### A.3 Confirmed ordinal → GL function mapping

| Ordinal   | Function                 | Key args / evidence |
|-----------|--------------------------|---------------------|
| `GL:4`    | glTexParameteri         | r0=GL_TEXTURE_2D; called once/texture before upload (and per-frame bind in Mahjong) |
| `GL:12`   | glClear (init only)      | r0=0x4000=GL_COLOR_BUFFER_BIT |
| `GL:13`   | glClearColor (init only) | r0,r1,r2,r3 = 0,0,0,1.0 (black) |
| `GL:37`   | **glDrawArrays** ✓       | mode=7 batched quads; mode=5 `GL_TRIANGLE_STRIP` seen in Texas Hold'em |
| `GL:38`   | **glDrawElements** ✓     | Sims/Sudoku/Solitaire family: r0=mode=5, r1=count, r2=0x1403 `GL_UNSIGNED_SHORT`, r3=index pointer |
| `GL:40`   | enableClientArray        | r0=array_index |
| `GL:45`   | createTexture + bindTex  | r0=obj_tag, r1=texture-object descriptor; Tetris/Holdem layout: desc word1=GL_tex_name, word2=source_format, word4/5=w/h; Mahjong layout: desc word1=ptr to obj whose word2=tex_name, word4=packed dims, word9=pixel_ptr |
| `GL:99`   | **glTexImage2D** ✓       | r0=GL_TEXTURE_2D, r1=level=0, r2=GL_RGB/RGBA/ALPHA, r3=width; once/texture |
| `GL:125`  | prepareDraw              | r0=0, r1=1, r2=0, r3=state_ptr |
| `GL:137`  | setVertexArrayFormat     | r0=array_idx, r1=components, r2=GL_FIXED, r3=stride, stack:ptr |
| `GL:157`  | **submitFrame**          | r0=0, r1=0, r2=5, r3=ptr; LAST call in aux |
| `GL:158`  | **presentFrame / swap**  | r0=0x3f001 (token), r2=work_ram_ptr; FIRST call in aux |
| `GL:159`  | bindTexture + vtxSetup   | r0=GL_tex_name (small int), r1=vtx_buf_ptr, r2=float |
| `GL:165`  | beginFrame / bindContext | r0=ctx, r1=ptr, r2=vtx_buf_ptr; once/frame after present |
| `GL:169`  | setPosition / translate  | r0=ctx, r1=x_float, r2=y_float, r3=0; screen-space coords |
| `GL:175`  | bindDrawState            | r0,r1,r2=static state ptrs |
| `GL:36`   | postDraw cleanup         | r0=1 or 2 |

### A.4 Texture dimensions from upload calls

GL:99 (glTexImage2D) called once per asset during loading phase (frame 2):

```
320 × 240  GL_RGB    screenBG_565.pix   (full-screen background)
 50 ×  50  GL_RGBA   eaLogo / small sprite
250 × 162  GL_RGBA   tetrisLogoT and similar
784 ×  20  GL_ALPHA  font-strip atlas ×3 variants (wide, thin, 1-bpp)
```

The `.pix` header is approximately 72 bytes before the raw pixel data
(153672 − 320×240×2 = 72 for the screenBG). The guest parses this header
itself after receiving bytes via `AsyncFileIO:3`.

### A.5 Per-frame GL call sequence (steady-state, ~4 quads/frame)

Double-buffered render loop confirmed from frame-40 GL trace:

```
aux(frame N):
  GL:158  — presentFrame(token=0x3f001, 1, vtx_buf_ptr)
             display frame N-1; FIRST call in aux
  GL:165  — beginFrame(ctx, ptr, vtx_buf_ptr)

  [repeated ×4 per frame:]
    GL:169  — setPosition(ctx, x, y, 0)    [x/y = screen-space float coords]
    GL:159  — bindTexture(tex_id, vtx_buf, size)
    GL:137  — setArrayFmt(0, 4, GL_FIXED, stride, ptr)  [position: XYZW]
    GL:40   — enableArray(0)
    GL:137  — setArrayFmt(1, 2, GL_FIXED, stride, ptr)  [texcoord: ST]
    GL:40   — enableArray(1)
    GL:175  — bindDrawState(state_ptr, vtx_arr_ptr, ctx_ptr)
    GL:125  — prepareDraw(0, 1, 0, state_ptr)
    GL:37   — glDrawArrays(GL_QUADS=7, first=0, count=4)   ← DRAW
    GL:36   — postDraw(1 or 2)

  GL:157  — submitFrame(0, 0, 5, ptr)
             commit frame N for presentation; LAST call in aux
```

### A.6 Key facts for Option B

- Vertex format: **GL_FIXED (16.16 fixed-point)**, not floats.
  4-component position (XYZW), 2-component texcoord (ST).
- Texture names are small integers (3, 14, 19, 27, ...) allocated sequentially.
- All texture pixel data is already in guest work RAM (delivered by
  `AsyncFileIO:3` to `[req+0x14]` buffers). The guest parsed .pix headers and
  the raw pixel data is at a known offset within those buffers.
- `GL:175`, `GL:125`, `GL:36` can be no-op stubs initially.
- `GL:12`, `GL:13` (clear/clearcolor) are init-only (2 calls total);
  can fill host framebuffer black on beginFrame instead.

---

## Option B: minimum viable GL ES subset (now the active milestone)

Not "implement all of GL ES 1.1" — implement exactly these ordinals:

**Priority 1** — required for any pixels:

1. `GL:45 + GL:4 + GL:99` — texture upload. Build a host-side texture cache
   keyed by GL name. Formats: GL_RGB/GL_RGBA/GL_ALPHA; dimensions from GL:99.
2. `GL:169` — setPosition(x, y). Track current sprite position.
3. `GL:159` — bindTexture + setVertexBuffer. Route to texture cache entry.
4. `GL:37`  — drawArrays. Rasterize: sample texture using GL_FIXED texcoords,
   write pixels to a 320×240 host framebuffer.
5. `GL:158` — presentFrame. Blit host framebuffer to minifb window.

**Priority 2** — correct quad geometry:

6. `GL:137 + GL:40` — vertex/texcoord array format + pointer. Decode GL_FIXED
   arrays: 4-component XYZW position, 2-component ST texcoord.
7. `GL:157` — submitFrame. Mark frame ready for next present.
8. `GL:165` — beginFrame. Clear host framebuffer.

**Priority 3** — correctness polish (not needed for first pixels):

- `GL:175`, `GL:125`, `GL:36` — stub as no-op
- Correct alpha-blending for `_a8` / `_4444` / `_5551` textures
- Save-data read/write (empty = new game, playable without it)

### Milestone 2: minimal runtime services

Prioritize stubs/HLE for the imports that repeatedly show up:

- `AsyncFileIO`
- `InputEvents`
- `Settings`
- `Metadata`
- `Audio` (stub acceptable at first)
- `OpenGLES` (likely software-backed shim / command adapter)
- `Filesytem` / `miscTBD` as discovered

### Milestone 3: first playable title

Target **Tetris** first.

Success bar:

- executable enters main loop
- assets load from the bundle
- input can move through menus / game state
- screen updates render in a host window
- save data can be written somewhere under a host-side per-game directory

## Open questions

- what exactly is the `eapp` header contract beyond the obvious first words?
  *(partially answered: entry/init/aux confirmed; words 6-7 at offsets 0x1c/0x20
  are still zero across all titles, unknown purpose)*
- how are imports resolved at runtime? *(answered: patched literal tables; stubs
  at `stubs_addr` do `ldr pc, [lit]`; we patch the lit to our trampolines)*
- ~~is `OpenGLES` a literal GL-style API, a command buffer, or just Apple naming?~~
  **Answered:** it is standard GL ES 1.1 with Apple's proprietary ordinal
  numbering. Standard format constants (GL_RGBA=0x1908, GL_FIXED=0x140C etc.)
  are confirmed.
- how much of the `*.sinf` / DRM story is already bypassed by the supplied
  firmware and bundles? *(still unknown; games run without DRM checks so far)*
- which services are pure userspace runtime ABI vs which ones implicitly depend
  on RetailOS kernel behavior? *(still unknown; GL/IO seem pure userspace)*
- what is the exact `.pix` file header format? *(72-byte header before raw
  pixels; structure not yet decoded — not needed since guest parses it itself)*
- what does `GL:157` r2=5 mean? *(unknown; could be quad count or a flags field)*
- what exactly does `GL:165` bind? *(context + vertex buffer ptr, but the
  double-ptr indirection via static globals is not fully traced)*

## Audio HLE: ABI investigation (2026-06-20)

Motivation: audio is reachable from **11 of 16 titles** (Tetris/PAC-MAN/Cubis2
all hit ~9 distinct `Audio:*` ordinals), and `.wav` assets are standard PCM —
so audio would be the first non-visual output `clicky` produces and is
verifiable via waveform/spectrum tooling rather than vision.

Investigation approach: added an env-gated `CLICKY_AUDIO_TRACE=1` diagnostic
(`clicky-core/src/sys/eapp/mod.rs::trace_audio_call`) that dumps, for every
`Audio:*` import, the register args plus a 64-byte preview of any arg that looks
like a guest pointer, with a RIFF/WAVE sniff + one-line WAV format parse
(`describe_wav_at`) when the pointed region begins with `RIFF....WAVE`. The
tracer is **guest-visible no-op** (returns 0 like the existing stub) and is
left in-tree as a reusable ABI-discovery tool. Artifact root:
`/tmp/clicky_audio_trace/`.

### Import call frequency (6M-cycle headless, 16 titles)

| Audio-reachable titles | OpenGLES | Audio | Metadata | InputEvents | Settings | AsyncFileIO | miscTBD |
|---|---:|---:|---:|---:|---:|---:|---:|
| Tetris `66666`     | 20 | 9 | 2 | 1 | 1 | 154 | 7 |
| Cubis 2 `99999`    | 15 | 9 | 2 | 1 | 1 | 241 | 4 |
| Holdem `33333`    | 19 | 5 | 2 | 1 | 1 | 57  | 4 |
| iQuiz `11002`     | 6  | 7 | 8 | 1 | 1 | 0   | 4 |
| PAC `AAAAA`        | 14 | 4 | 0 | 1 | 1 | 61  | 6 |
| Ms.PAC `14004`     | 14 | 3 | 0 | 1 | 1 | 81  | 4 |
| Mini Golf `88888`  | 17 | 3 | 0 | 1 | 1 | 9   | 4 |
| LOST `1B200`       | 9  | 3 | 0 | 1 | 1 | 6   | 3 |
| Zuma/SimsBowl/etc  | —  | ≤1 | 0 | 1 | 1 | —   | — |

**11 of 16 titles call Audio.** The full deduped ordinal set across titles is:
`Audio:0, 2, 5, 13, 14, 15, 40, 43, 45, 47, 48, 49, 50, 51, 52, 53, 55, 56, 60`
(~19 distinct ordinals, small and enumerable).

### Confirmed Audio ordinal → role mapping (startup path only)

Decoded from Tetris `66666` traces. The **play** ordinals (which carry the PCM
bytes) were not captured — see the blocker note below.

| Ordinal | Inferred role | Key args / evidence |
|---|---|---|
| `Audio:0`  | allocator / zero-fill object-table slot | `r0=1` (count), `r1=0..9` (slot index), `r2=0x18025ec8` (fixed work-RAM table base). Each call extends the zeroed region by ~16 bytes; called 10× to initialize a 10-slot table (mixer channels). |
| `Audio:40` | allocate buffer of `r3` bytes | `r0=stack_ptr` (out: handle), `r3=0xdea8`=57000 (byte size). Preview at `r0` is zero (uninitialized out-param). |
| `Audio:52` | register/bind a source descriptor | two shapes: `r0=0x101a7830` (ptr→`0x18023ea4` (file-VMA fn ptr) + `0x1` flag), `r1=source_id`; OR `r0=0, r1=0x437f0000` (float 255.0), `r2=0x9e`. |
| `Audio:51` | source play/stop control | `r0=source_id`, `r1=action_flag`. |
| `Audio:56` | queue buffer on source | `r0=1, r1=0xa` (buffer id 10), `r2=0x10001560` (work-RAM, **zeroed at call time — WAV not yet staged here**), `r3=0x180232af` (ASCII `"Abnormal termina..."` — a debug *name string*, not PCM). |
| `Audio:55` | config / set stream property | `r0=0x7800`=30720 (buffer-period-size? sample count?), later calls use float args (1.0f). |
| `Audio:53` | master volume set (byte→fixed) | `r0=0`, `r1=0x80808081` (classic divide-by-255 reciprocal — 0..255 byte → 0..1 fixed-point scaling). |
| `Audio:48` / `Audio:43` | set per-source gain (float) | `r1=r2=r3=0x3f800000` (1.0f — unity gain default). |
| `Audio:13/14/15/2` | start-game audio setup | appear only after `action` (Select) input; `r1=r2=0x7800`, `r3=0x18025ec8` (the `Audio:0` object table). |

### Critical data-path finding: WAV bytes never appear directly in Audio args

The tracer sniffed for RIFF/WAVE at every pointer arg across **8 audio-heavy
titles and found zero direct WAV pointers.** The `.wav` files on disk are real
full PCM (e.g. `Clear.wav` = 132KB, RIFF/WAVE, 16-bit stereo 22050 Hz), but
`AsyncFileIO:3` reported delivering only **44 bytes per `.wav`** — i.e. the
*guest requested just the WAV header* (44 bytes) to parse the format, then
fetches the PCM data chunk via separate offset-based `AsyncFileIO:3` requests
(which the host also fulfills). The PCM ends up in guest RAM at a
`staged_files`-tracked address, but the Audio runtime references it **via a
buffer-handle indirection** (`Audio:40` alloc → `Audio:56` queue), not via a
raw pointer in an Audio arg.

So accurate audio playback requires resolving the Audio buffer-handle →
staged-PCM mapping, which in turn requires seeing the *play* ordinals fire
with a real buffer handle bound.

### Blocker: no title reaches the audio play path

The play-path `Audio:*` ordinals (the ones that would reveal the
buffer-handle → PCM binding) only fire on **gameplay events** (Tetris
`Menu`/`Move`/`Drop`/`Clear` SFX). `CLICKY_EAPP_INPUT_SCRIPT` was used to
inject `menu` and `action` (Select) presses to trigger them headlessly —
**both crash Tetris before any play ordinal fires**:
- `menu:60-65` → `FatalMemException pc=0x180206fc kind=Read off=0x14
  in_device="eapp, <unmapped>"` (null object deref, offset 0x14)
- `action:60-65` → same fatal at `pc=0x180206fc off=0x14` (after reaching
  `Audio:13/14/15/2` start-game setup, then null-deref)

This is the **same null-object-provider family** already documented for
iQuiz (`pc=0x18001b08 off=0xc`) and Vortex (`pc=0x18014d58 off=0x4`), now
also blocking Tetris gameplay-start. The guest reads a field at `[object+0x14]`
where `object` is null — an upstream HLE call (likely a `Metadata` object
provider, currently stubbed `return 0`) should have populated it.

### Implications for what to work on next

Audio playback cannot be verified end-to-end on Tetris (or any title) until a
title can reach gameplay. The accuracy-first sequence is therefore:
1. **Fix the null-object-provider fatal** (`pc=0x180206fc off=0x14` and
   siblings) so Tetris (and iQuiz, Vortex) can start/play. This is a
   runtime/ABI fix, not audio work, but it unblocks audio verification,
   iQuiz + Vortex (2 currently-dead titles), and makes Tetris playable —
   the single highest-leverage remaining item.
2. **Capture the play-path Audio ordinals** with input injection once
   gameplay is reachable — the tracer already in-tree will reveal the
   buffer-handle → PCM binding generically.
3. **Build the audio HLE** (context/buffer/source object model + cpal host
   sink in `clicky-desktop`, WAV parse via `hound`) against the now-complete
   ordinal table, validated by Tetris SFX.

No audio HLE implementation was written this round — the investigation
established that doing so blind (without the play ABI) would necessarily be
Tetris-specific guesswork, contradicting the "generic and accurate" goal. The
tracer + this ABI table are the reusable deliverables; the null-object-provider
fix is the unblocker.

## Null-vtable teardown crash: investigation (2026-06-20)

Motivation: the null-object-provider fatal is the single highest-leverage
remaining item — it blocks Tetris gameplay (and thus the audio play-ABI
capture), plus iQuiz and Vortex (two currently-dead titles). All three were
previously grouped as "null-destination buffer/object pointer that an upstream
HLE call should have populated," but the specific owning import was unproven.
This investigation dissects the Tetris `66666` case concretely.

### Reproduction

Reproduced deterministically (5/5 runs, identical register state) with:
`CLICKY_EAPP_INPUT_SCRIPT="menu:50-120"` + the standard live-GL env, 12M
cycc headless. Note: the crash fires on the **Menu** button (exit-to-shell
path), NOT on Select/Action — Select at the wrong guest frame does nothing, and
Select during menu-select hits a *different* fatal (`pc=0x180206fc` offset 0x14,
but only after reacing `Audio:13/14/15/2`); the reliable trigger is Menu-hold.

The faulting object address is **stable across runs**: `r0=0x100e4be0`,
`r5=r6=0x1019f7f0` (list anchor), `r7=0x3` (count). Determinism of the
allocation pattern (modulo the wall-clock-driven `miscTBD:9` time API, which
affects timing not addresses) lets a faulting object be re-identified across
runs.

### Crash mechanism (disassembled)

Faulting instruction: `0x180206fc` (`bx r1`). The fault is a Read at
`offset=0x14` (unmapped). Decoded ARM at `0x206b0..0x206fc`:

```
0x206b0: cmp r0, #0          ; r0 = object ptr (the list node)
0x206b4: bxne lr             ; early-out if null
0x206b8: push {r4-r8, lr}
0x206bc: subs r7, r1, #0     ; r7 = count (from caller r1 = 3)
0x206c0: mov r6, r0          ; r6 = array base
0x206c4: mov r4, #0          ; r4 = index
0x206c8: ble ...              ; if count<=0 skip
0x206d0: add r5, r6, r4, lsl #2  ; r5 = &array[i]  (4-byte ptr table)
0x206d8: ldr r0, [r5]        ; r0 = array[i] = object ptr
0x206dc: cmp r0, #0
0x206e4: ldr r1, [r0, #4]    ; r1 = refcount (object word 1)
0x206e8: subs r1, r1, #1     ; refcount--
0x206ec: str r1, [r0, #4]
0x206f0: ldreq r1, [r0]      ; if refcount==0: r1 = vtable (object word 0)
0x206f4: addeq lr, pc, #4
0x206f8: ldreq r1, [r1, #0x14] ; r1 = vtable[5] (destructor slot)
0x206fc: bxeq r1             ; call destructor   <-- FAULT: r1 (vtable) was 0
```

So this is a **generic release-list function** at `0x206b0`: takes
`(r0=array_ptr, r1=count)`, iterates an array of object pointers, drops each
one's refcount, and dispatches `vtable[5]` (offset `0x14`) as the destructor for
any whose refcount hits zero. The list anchor at `0x1019f7f0` holds
`[0x100e4be0, 0x100e4e00, 0x100e5020, 0, ...]` — three 544-byte objects spaced
`0x220` apart.

The faulting node's memory dump (added a generic `fault object @r0` dump to
the fault handler this round):
```
fault object @r0 words=[0x00000000,0x00000000,0x100e4c00, 0×13 zeros]
fault object sibling @r0[2]=0x100e4c00 words=[all zeros]
fault list anchor @r5=0x1019f7f0 words=[0x100e4be0,0x100e4e00,0x100e5020, 0×5 zeros]
```

The faulting node is `{vtable=0, refcount=0, next=0x100e4c00, zeros}`. Note:
for the destructor to fire at all, `refcount` must have been written to exactly
`1` at some earlier point (so `1-1=0` sets the `ldreq` condition). So the
object's **refcount WAS initialized** but its **vtable was never written**.

### Call path into the crash

The recent-pc-trace shows the release-list loop (`0x206b0..0x206fc`) is
called from `0x7868: bl 0x206b0`, whose caller at `0x783c` does:
```
0x7854: str r0, [r4]         ; writes the CONTAINER's vtable from a literal
0x7858: ldr r1, [r4, #0x54]  ; container field at +0x54 (count)
0x7864: ldr r0, [r4, #0x5c]  ; container field at +0x5c (array ptr) = the list
0x7868: bl 0x206b0           ; release the contained array
```
So the container's own vtable IS set properly (from a game-image literal); the
three *contained* nodes are the ones missing vtables. This is a one-level-deep
init gap, not a wholesale broken object.

### Imports fired immediately before the fault

From the full log (~40 lines before fatal), the Menu-press teardown sequence
is:
1. `AsyncFileIO:3` re-reads every `.wav` header (44 B each) — preparing to
   release audio buffers
2. `AsyncFileIO:3` re-reads `prefs.sav` / `game.sav` (saves on exit)
3. `OpenGLES:158` (present) + `candidate_begin double-begin detected`
4. `Audio:52/51/56/40/55/53/48/43` (audio teardown / source release)
5. **`Metadata:62 r0=0xffffffff r1=0x101a9a18 r2=0xb r3=1.0f`**
6. **`Metadata:134 r0=0 r1=0x101a9a18 r2=0xb r3=1.0f`**
7. `miscTBD:14` (block copy), `Audio:40` (allocate), `miscTBD:12`
8. → release-list loop → fault on node `0x100e4be0`

`Metadata:62` and `Metadata:134` both go to the **stubbed** `Metadata` module
(`return 0`). Both share `r1=0x101a9a18` (work-RAM context) and `r2=0xb`. This
keeps the earlier "a `Metadata` object-provider is implicated" hypothesis alive
(iQuiz's fatal was attributed to `Metadata` too) — but see the next finding,
which complicates it.

### Key finding: the faulting object is NOT from the main HLE allocator

Added an env-gated `CLICKY_ALLOC_TRACE=1` log of every `miscTBD:0` allocation's
`(caller_lr, returned_addr, len)`. Results:

- **`0x100e4be0` does not appear in any `miscTBD:0` return.** Every logged
  allocation returns addresses in `0x101d42d0..0x101e11c0` (the post-bootstrap
  heap region). The faulting object at `0x100e4be0` lives in the *earlier*
  `0x1000_1000..0x101d_42d0` region — ~1.9 MiB of work-RAM that was consumed
  *before* the first logged `miscTBD:0` call (the allocator's `next_alloc`
  was already at `0x101d42d0` when logging began).
- Every single `miscTBD:0` call shares `lr=0x18021b68` — the runtime funnels
  all allocations through one allocator trampoline, so `lr` does **not**
  distinguish allocation sites. A per-site attribution would need either the
  trampoline's caller (one frame up the stack) or a memory-write watchpoint.

**Implication:** the faulting object is either a bootstrap/early reservation
or a sub-allocation the game carves from a large early arena (a common pattern:
the runtime hands the game one big arena, and the game runs its own malloc on
top). If the latter, the object's vtable would be written by **game code**, not
by an HLE stub — which would make the crash a "game expects the OS to handle
part of the teardown" issue rather than a simple "stub forgot to fill in the
vtable" fix.

This is consistent with the doc's prior note that the Menu-press crash happens
because there is "no RetailOS shell to return to" — i.e. the game's exit path
expects the OS to participate in the teardown, and our eapp spike has no OS.

### Candidate fix shapes (not yet implemented)

1. **Populate the missing vtable via an HLE init call.** Only viable if
   `0x100e4be0` turns out to be an HLE-allocated object whose constructor
   lives in a stubbed module. The `Metadata:62/134` calls right before the
   fault are the leading candidates, but we have no RetailOS RE to tell us
   what object they should return. This is the "accuracy" path but needs
   reference RE we don't currently have.
2. **Intercept the exit-to-shell request.** Detect (via the Menu-hold
   `InputEvents` signal, or a specific `miscTBD`/`Metadata` ordinal the game
   uses to signal "quit") when the game wants to exit, and either halt the
   eapp cleanly or short-circuit the doomed refcount teardown. This is
   "graceful degradation" rather than true emulation accuracy, but it is
   honest about the no-OS reality and would stop the crash deterministically.
3. **Trace the object's constructor via a memory-write watchpoint.** Add a
   generic `CLICKY_EAPP_WATCH=0xADDR,0xLEN` env-gated watch on
   `0x100e4be0..+0x220` to find (a) who sets `refcount=1` (word 1) and (b)
   whether anything ever writes word 0. If word 0 is written by a guest
   literal load, the object is game-owned and fix-shape #2 applies; if word 0
   is meant to be written by an HLE return, fix-shape #1 applies.

The next concrete step is #3 (the write-watch), to decide between #1 and #2.

### Decisive finding from the write-watch (rules out fix #1)

Implemented the `CLICKY_EAPP_WATCH=0xADDR,0xLEN` env-gated write-watchpoint
(generic RE infrastructure: `EappBus` carries an optional `(start, end)`
range + `pending_pc` updated each `step()`, and `w32` records every in-range
write as `(addr, val, pc)`; dumped in the fatal path). Ran the Menu crash with
`CLICKY_EAPP_WATCH=0x100e4be0,0x660` (covers all 3 list nodes):

```
watch hits (13 total):
  write addr=0x100e4be8 val=0x100e4c00 pc=0x18013de0   ; node0.next
  write addr=0x100e4e08 val=0x100e4e20 pc=0x18013de0   ; node1.next
  write addr=0x100e5028 val=0x100e5040 pc=0x18013de0   ; node2.next  ← list constructor links all 3
  write addr=0x100e4be4 val=0x00000001 pc=0x18020650   ; node0.refcount=1
  write addr=0x100e4e04 val=0x00000001 pc=0x18020650   ; node1.refcount=1
  write addr=0x100e5024 val=0x00000001 pc=0x18020650   ; node2.refcount=1  ← retain/adopt
  write addr=0x100e4be4 val=0x00000002 pc=0x18020650   ; refcount 1→2
  ... (all 3 nodes retained twice)
  write addr=0x100e4be4 val=0x00000001 pc=0x180206ec   ; release loop 2→1
  ... (all 3 released)
  write addr=0x100e4be4 val=0x00000000 pc=0x180206ec   ; release loop 1→0 → dtor → FAULT
```

**The object's word 0 (vtable) is NEVER written by anything.** Zero hits at
`0x100e4be0`/`0x100e4e00`/`0x100e5020` (the vtable fields). The only writers
are:
- `0x18013de0` — the list constructor, writes the `+0x8` next-link
- `0x18020650` — a `retain(obj, out_ptr)` routine (`ldr r2,[r0,#4]; add r2,#1;
  str r2,[r0,#4]; strne r0,[r1]`), writes the `+0x4` refcount
- `0x180206ec` — the crash loop's own `str r1,[r0,#4]`, decrementing refcount

Decoding around the writers revealed the decisive structural fact: **there are
TWO release functions**, and only one guards against null vtables:

- `0x1802066c` — single-object `release(obj)`: `ldr r0,[r0]` (load vtable),
  **`cmp r0,#0; beq skip_dtor`** (explicitly skips the destructor if
  `vtable==0`), then `ldr r1,[r4,#4]; subs r1,#1; str r1,[r4,#4]; ...`.
  This path **tolerates** null-vtable objects as a legitimate state.
- `0x180206b0` — the list-release **loop** (the one we crash in): same
  refcount-drop + `vtable[5]` dispatch, but **no null-vtable guard**.

### Conclusion: fix shape #1 is wrong; fix shape #2 (model the OS exit) is correct

The game **deliberately never sets these vtables** — they are zeroed by the
allocator and left at 0. The safe single-release path anticipates and tolerates
`vtable==0` (skip-dtor). The crash comes from running a *different*, un-guarded
list-release loop that is only reachable via the Menu-hold **exit-to-shell**
path — which on real hardware never runs, because RetailOS kills the game
process the moment it signals exit, before the game gets to walk its own
self-destruct path.

This means:
- Fix shape #1 (have some HLE stub populate the vtable) would be **wrong** —
  the game doesn't expect these objects to have vtables; it expects to be
  killed. Inventing a vtable would be guessing at a destructor that real
  hardware never invokes.
- Fix shape #2 (model the OS process-kill on exit) is the **accurate** fix:
  it does exactly what real hardware does — the game signals "exit," the OS
  terminates the process, and the post-exit teardown code never runs. Since
  the eapp spike has no RetailOS, the eapp runtime itself must serve as the
  OS-kill.

The remaining open question for fix #2 is **how the game signals "exit to OS"**
so we can intercept it cleanly rather than pattern-matching Tetris-specific
teardown. Candidates to investigate next:
- A specific `miscTBD` ordinal meaning "exit process" (the run shows
  `miscTBD:12` and `miscTBD:14` firing in the teardown, with `r3=0x18002c5c`
  and `r1=0x13ffecd8` — candidate exit/signal calls)
- The Menu-hold `InputEvents:0` return path (the game may pass a quit event up)
- The `Metadata:62`/`Metadata:134` calls right before the crash (shared
  `r1=0x101a9a18`, `r2=0xb` — possibly a "flush and exit" metadata op)

### Diagnostics added this round (both permanent, env-gated, generic)

- `fault object @r0` dump: on every fatal, reads 16 words at `r0` plus any
  work-RAM pointer-field siblings and the `r5==r6` list anchor. Surfaces the
  object layout + sibling vtables + which-node-in-list faulted, without
  needing to re-run with custom instrumentation. Fires only in the fatal
  path so it costs nothing in normal runs.
- `CLICKY_ALLOC_TRACE=1`: logs every `miscTBD:0` allocation's
  `(caller_lr, returned_addr, requested_len)`. Generic RE tool for
  attributing any future faulting work-RAM object to an allocation (modulo
  the trampoline-funnel caveat noted above).

## Working rule for this branch

When in doubt, prefer:

- **game-facing runtime shims**
- **host-native replacements**
- **small focused instrumentation**

…over broad hardware modeling that does not move a real game closer to booting.
