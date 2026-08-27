# Decrypted clickwheel-game interactive matrix

Date: 2026-08-26 UTC; renderer checkpoint: 2026-08-27 UTC
Repository: `fliwheel`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`
Runner: `target/release/eapp` with experimental live GL HLE  

This is the corrected corpus-wide scripted interaction pass. It is deliberately
more conservative than the older `WORKS` labels: a title is not playable until
the runner reaches content, accepts meaningful controls, preserves a coherent
frame, and has a title-specific sound/persistence check. The automated probe
only supplies a common button/wheel schedule, so these results are triage
evidence and not completion claims.

## Reproduction

The reusable harness is:

```bash
CYCLES=30000000 \
RUN_ROOT=/tmp/fliwheel_interactive_corrected_20260826 \
./scripts/test_decrypted_games_interactive.sh \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO'
```

The earlier family reports are in `/tmp/fliwheel_interactive_full_{a,b,c,d}`;
they predate the title-scoped completion correction. The canonical corrected
report is `/tmp/fliwheel_interactive_corrected_20260826/interactive_matrix.md`.
The post-PopCap checkpoint regression at the same 30M-cycle budget is
`/tmp/fliwheel_postpopcap_matrix_20260826/interactive_matrix.md`; all 20
bundles exited cleanly with zero fatal signatures.
The latest post-bind-state regression is
`/tmp/fliwheel_iter3_matrix_20260826/interactive_matrix.md`; it also covers
all 20 bundles at 30M cycles with zero fatal signatures.
That corpus-wide report predates the default PopCap `.ro` byte-count fix. The
current PopCap pair regression is recorded in
`/tmp/fliwheel_bejeweled_startup_default_20260826/` and
`/tmp/fliwheel_zuma_startup_default_20260826/` and is summarized in the
[PopCap contract probe](20260826_popcap_dma_contract.md).
It includes a per-title log and PPM captures. The run used:

```text
action:18-20, action:45-47, action:80-82, action:130-132,
wheel=3:180-182, left:230-232, right:280-282,
up:330-332, down:380-382, action:430-432
```

After the title-specific audio guard and Sims geometry classification were
committed, the focused regression was rerun at 10,000,000 cycles for
`1500C`, `1500E`, `33333`, `50514`, and `66666`. That earlier report is
`/tmp/fliwheel_postchange_probe/interactive_matrix.md`. It enabled the
experimental Tetris parsed-resource completion override for every bundle;
that is not a valid cross-title contract. The harness now enables that
override only for `66666`. A corrected 30,000,000-cycle Hold'em run is at
`/tmp/fliwheel_holdem_matrix_20260826/interactive_matrix.md`.

`Unique hashes`, `hash changes`, and draw counts below come from each capture
manifest. They measure visual activity, not correctness.

## Current matrix

| Bundle | Title | Evidence from scripted run | Current assessment | Next gate |
| --- | --- | --- | --- | --- |
| `11002` | iQuiz | 700 guest frames, one black framebuffer hash, one skipped 149x75 A8 draw; the guest asks for a `data` pack after directory setup | Blocked at pack/content loading | Reconstruct the iQuiz pack index and map its material upload |
| `11050` | SAT Prep Reading | 4 frames, 2 hashes, 2 draws; recognizable SAT Prep splash with spinner | Splash/loading only | Drive the menu after the data/font path is understood |
| `11051` | SAT Prep Writing | 4 frames, 2 hashes, 2 draws; recognizable SAT Prep splash with spinner | Splash/loading only | Same shared testprep path as `11050` |
| `11052` | SAT Prep Mathematics | 4 frames, 2 hashes, 2 draws; recognizable SAT Prep splash with spinner | Splash/loading only | Same shared testprep path as `11050` |
| `12345` | Vortex | 700 frames and continuously changing hashes; recognizable Vortex title art, but no content scene | Animated splash/title only | Decode the VBO/vertex indirection around ordinals 175/125 |
| `14004` | Ms. PAC-MAN | 700 frames, 2 hashes, up to 12 draws; recognizable Namco loading art, with visible text/texture artifacts | Loading screen only | Validate texture atlas selection and advance past loading |
| `1500C` | The Sims Bowling | 700 frames, 2 hashes, at most two draws; framebuffer is effectively black. The current HLE recognizes its normalized geometry family, but useful coverage remains absent | Renderer/asset decode blocked | Decode the `gameLib.rlb`/rserver texture path and verify NDC coverage |
| `1500E` | The Sims Pool | 700 frames, 2 hashes, at most two draws; same black/zero-coverage shape as Bowling | Renderer/asset decode blocked | Share the Bowling fix, then exercise wheel aim/power/spin |
| `1B200` | LOST | 700 frames, zero GL draws, blank framebuffer; rserver loads but no shader output | Shader/render-server blocked | Parse or emulate the `rserver.bin` programmable path |
| `1C300` | musika | One captured frame with the Musika logo; no fatal | Splash only | Reverse the animation/packet/sound-bank runtime after the first scene |
| `33333` | Texas Hold'em | Corrected run completes 30,000,000 cycles / 700 captured frames with 3 visual hashes, 34 draws per steady-state frame, and no fatal; the stabilized capture is the `LOADING` screen | Loading screen only; playable state not reached | Reverse the title-specific resource completion/transition path, then verify poker-table controls and sound |
| `44444` | Zuma | Deterministic entry reaches the Temple, all built-in instruction pages, tutorial close, and live `LEVEL 1-1: SPIRAL OF DOOM`; commit `b12cd60` renders the spiral path and colored marbles coherently, and a durable 400M-cycle probe shows a post-entry projectile, collision/result animation, and bonus coin with no fatal signature | One fire/collision sequence verified; repeatable aim, audio, and save remain open | Repeat wheel-to-aim/fire several times, then verify audio and persistence |
| `50513` | Sudoku | Recognizable title screen; reversed-register event heads now drain cleanly, and Menu enters the save/settings teardown loop. An opt-in full RLB completion runs its callback chain but adds no board draws | Input lifecycle and Menu/exit path verified; board not reached | Derive the puzzle-start/select contract and render the puzzle grid |
| `50514` | Royal Solitaire | 700 frames, 26 hashes and 59 changes; the character splash is spatially coherent, scripted event nodes are consumed, and the savefile request completes | Coherent splash; readiness/object contract unresolved and board not reached | Reconstruct the manager readiness callback, then validate card/selection interaction |
| `55555` | Bejeweled | Normalized wheel input reaches the tutorial and live 8×8 board; a deterministic `[8,6]` to `[8,7]` swap enters the guest swap path, displays “EXCELLENT!”, changes/refills tiles, and opens the score-bar overlay; the runner maps arrow keys to the four tap quadrants | Single live match verified; headed/mode/audio/save coverage remains | Confirm the arrow-key gesture in a headed run, then repeat across modes and verify audio/persistence |
| `66666` | Tetris | Corrected run reaches frame 501 with 20 hashes and up to 382 draws; the separate targeted schedule reaches the board, pause/resume, left/right, hard drop, and indexed `Menu.wav`/`Move.wav`/`Drop.wav` events | Best current target; interactive partial, not complete | Finish wheel displacement, line clears, persistence, long-run visual parity, and sound mixing |
| `77777` | Mahjong | 700 frames, 71 hashes, up to eight draws; mostly dotted/garbled title output | Texture/UV partial | Decode the `main.rlb` resource path and tile atlas |
| `88888` | Mini Golf | 700 frames, two hashes, five draws; mostly black with a loading/progress outline | Splash/loading only | Load the compact course resources and reach the menu |
| `99999` | Cubis 2 | 700 frames, 26 hashes and 27 changes, up to 49 draws; black/loading state despite many staged assets | Asset/renderer blocked | Decode the `.raw`/`.pix` image path and material handles |
| `AAAAA` | PAC-MAN | 425 frames, two hashes, up to 58 draws; recognizable Namco loading screen, content not reached | Loading screen only | Advance through menu and verify maze/D-pad movement |

No row is marked fully playable. Tetris and Bejeweled now have content-level
control evidence, but both remain incomplete under the project goal.

## Scoped Hold'em progress (excluded from the default matrix)

The default `33333` row intentionally remains loading-only. A separate,
title-scoped experiment now completes the Hold'em resource callback sequence,
decodes the title's `GL_PALETTE8_RGBA8_OES` indexed artwork, and uses the
name-entry wheel/action path to reach the first post-name scene. The sweep
reaches 113 draws at guest frame 553; the detailed rerun reaches a 107-draw
scene at guest frame 607 before later blank/partial transitions. The scene is
not yet coherent or playable, and these overrides are not part of the
corpus-wide contract:

```text
EAPP_TEXAS_ASYNC0_COMPLETE=1 EAPP_TEXAS_ASYNC0_STATUS=1
EAPP_TEXAS_ASYNC2_COMPLETE=1 EAPP_TEXAS_ASYNC1_COMPLETE=1
```

Evidence is retained at `/tmp/fliwheel_holdem_ok_sweep_20260826/` and
`/tmp/fliwheel_holdem_table_20260826/`.

## Shared changes made after this run

- The NDC detector now includes the empirically matching Sims Bowling/Pool
  family (`1500C`/`1500E`) in addition to Sudoku/Solitaire. A targeted rerun
  confirms the titles remain stable, but does not yet show useful coverage.
- The resource-indexed audio observer is now scoped to Tetris (`66666`). The
  first corpus run showed that the same virtual addresses are unrelated code
  in other binaries and produced fabricated event IDs; those counts are not
  valid audio evidence and are excluded from this matrix.
- `FLIWHEEL_EAPP_ASYNC3_COMPLETE=1` is now title-scoped in both the runtime and
  interactive harness. It is a provisional parsed-resource experiment for
  Tetris only; applying its completion fields to Hold'em reaches a different
  resource ABI and produced a misleading null-table fault.
- The InputEvents HLE now handles the reversed-register owner lifecycle used by
  Sudoku/Solitaire: it retains the owner across a dropped register, defers
  clearing one poll, and relinks later edge nodes. Focused regression remained
  clean for `1500C`, `1500E`, `50514`, and `66666`. Sudoku's title-scoped RLB
  completion probe is retained as negative evidence rather than enabled by
  default.
- The normalized-coordinate renderer now applies the shared 1.2x0.9 projection
  globally. Royal Solitaire's small character/UI quads no longer get stretched
  to the full viewport; the change was regression-tested against Sudoku, both
  Sims titles, and Tetris.
- The desktop host sink remains headless-unverified. Tetris has a persistent
  `rodio` sink and a guest-indexed event queue, but physical output, overlap,
  timing, and mixer parity still need a headed test and waveform regression.
- The reference eApp lifecycle's one-time init vector is implemented behind
  `FLIWHEEL_EAPP_INIT_VECTORS=1` with valid scratch contexts. It remains opt-in:
  forcing it exposed unresolved init/render-server contracts in `14004`,
  `1500C`, and `1500E`; the default matrix stays on the no-fatal path. See
  [the init-vector probe](20260826_eapp_init_vectors.md).
- The DMA-only present path now records PopCap frames in the same startup
  manifest as GL-backed frames. A focused 50M-cycle run confirms full-buffer
  writes for both Zuma and Bejeweled, but the final images remain malformed or
  partial. The firmware-reference SDRAM-alias model was tested opt-in and
  rejected after Bejeweled reproduced the final hash and then faulted. See
  [the PopCap DMA contract probe](20260826_popcap_dma_contract.md).
- PopCap `.ro` async reads now publish their actual byte count to the guest
  completion object by default. This fixes the measured negative final-resource
  length and moves both titles from the old DMA-only path to resource-backed GL
  scenes. The scenes are still visually partial; the new target is texture
  association/orientation and board composition. See [the PopCap DMA contract
  probe](20260826_popcap_dma_contract.md).
- The PopCap full-surface selector now accepts Zuma's measured 320x240 UV span
  against its valid 322x222 RGBA4444 board upload. The first 8M-cycle capture
  covers all 76,800 framebuffer pixels and visibly restores the stone-framed
  board; title/menu atlas placement remains unresolved. See the [PopCap DMA
  contract probe](20260826_popcap_dma_contract.md).
- PopCap draws now retain the guest's live OpenGLES:4 texture bind and prefer
  that texture name over the reused material handle when UV ranges overlap. A
  30M-cycle pair regression shows Zuma selecting uploads `7`, `6`, and `5` for
  distinct overlay roles and presenting an upright board. PopCap bundles now
  default to the guest screen origin, with `FLIWHEEL_GL_PRESENT_VFLIP` retained
  for explicit A/B tests. See the [PopCap DMA contract probe](20260826_popcap_dma_contract.md).
- PopCap upload association now replaces stale ordinal-45 metadata whenever a
  real OpenGLES:4 bind arrives. This fixes the observed Zuma `0x8` marble-atlas
  upload being incorrectly tagged as `0x7`; the corrected board capture and
  control replay are recorded in the [Zuma board-entry receipt](20260826_zuma_board_entry.md).
- The durable Zuma gameplay probe now reaches beyond board entry: a wheel
  rotation followed by Select produces a visible projectile, collision/result
  animation, score/chain change, and bonus coin. This verifies one input-to-
  gameplay path, while repeatable aim-angle control, audio, and persistence
  remain unverified. See the [Zuma gameplay probe](20260827_zuma_gameplay_probe.md).
- Executable discovery ignores archive-generated AppleDouble `._*.bin`
  sidecars. The durable corpus extracted from the external drive contains
  those metadata files alongside the real plaintext eApp binaries, so this is
  covered by every subsequent corpus run.
- The legacy `Filesytem` ABI now has independent synthetic handles,
  sequential host-backed reads, and close semantics. Bejeweled reaches its
  menu, 8×8 board, and “Selecting Gems” tutorial; normalized wheel input now
  drives a deterministic live-board match with visible refill and score-bar
  results. Headed tap input and title-specific audio/save checks remain open.
  See the [Bejeweled game report](../games/55555_bejeweled.md) and the
  [PopCap DMA contract probe](20260826_popcap_dma_contract.md).
- Bejeweled directional arrow presses are now translated into the title's
  normalized clickwheel touch-angle packets, while other bundles retain their
  ordinary button mapping. The scripted wheel delta is also applied once per
  guest frame even when a title polls InputEvents repeatedly.
- Royal Solitaire's readiness investigation confirmed that its event list is
  delivered and consumed and that its savefile request completes; the guest
  manager gate at `0x180cfa5c` remains at `2`. A diagnostic clear only tears
  down the manager and leaves the splash, so no unconditional gate patch was
  accepted. See [the readiness-contract probe](20260826_royal_readiness.md).

## External visual/control references

The preservation list and title coverage are cross-checked against the
[Clickwheel Games list on iPodWiki](https://ipodwiki.com/wiki/Clickwheel_games)
and the [2024 preservation-project index](https://www.reddit.com/r/ipod/comments/1g92jvj/).
For expected behavior, the [iLounge Tetris review](https://www.ilounge.com/index.php/reviews/entry/electronic-arts-tetris)
describes the well, wheel movement, side-button rotation, and hard drop; the
[Wikimedia Tetris-on-iPod screenshot](https://commons.wikimedia.org/wiki/File:Tetris_on_an_iPod.jpg)
is a visual reference for the 320x240 presentation. The preservation index
also links short gameplay references for [musika](https://www.youtube.com/shorts/WOL6uPZfnD8),
[PAC-MAN](https://www.youtube.com/shorts/tT6LZ3k60pI),
[Royal Solitaire](https://www.youtube.com/shorts/e9v10Xb09KY),
[Sudoku](https://www.youtube.com/shorts/LGksyLgYGOs),
[The Sims Bowling](https://www.youtube.com/shorts/RHHeqbbvQjs),
[The Sims Pool](https://www.youtube.com/shorts/gSPaxR0NQWc), and
[Vortex](https://www.youtube.com/shorts/wssyFxmw4jw).

These references are used as behavioral/visual targets, not as substitutes
for deterministic guest traces.
