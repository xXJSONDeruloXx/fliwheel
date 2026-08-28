# Decrypted clickwheel-game interactive matrix

Date: 2026-08-26 UTC; renderer checkpoint: 2026-08-27 UTC; follow-up probes:
2026-08-27 UTC
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

The focused release checkpoint for the Sims texture-target change is
`/tmp/fliwheel_checkpoint_20260827/interactive_matrix.md`; its Sims rows are
the current evidence below, while the other rows retain the corpus-wide
triage values from the earlier 20-bundle run.

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
| `11050` | SAT Prep Reading | Two completed resource callbacks (`rserver.bin`, `Fonts/Roman/fontinfo.txt`), then a coherent full-screen splash plus animated 34x34 spinner in steady state 6; Audio:47/input A/B did not transition it | Coherent splash/loading only; content handoff open | Identify the SAT content/runtime handoff |
| `11051` | SAT Prep Writing | Same two-resource loader and coherent splash/spinner path as `11050`; clean bounded exit | Coherent splash/loading only; content handoff open | Share the SAT handoff investigation |
| `11052` | SAT Prep Mathematics | Same two-resource loader and coherent splash/spinner path as `11050`; clean bounded exit | Coherent splash/loading only; content handoff open | Share the SAT handoff investigation |
| `12345` | Vortex | 700 frames and continuously changing hashes; recognizable Vortex title art, but no content scene | Animated splash/title only | Decode the VBO/vertex indirection around ordinals 175/125 |
| `14004` | Ms. PAC-MAN | Normal path completes the four measured async stages, reaches name entry/main menu/Play Game/tutorial, then reaches a controllable Stage 1 maze and advances the score 0→160 with raw clickwheel quadrant input; a bounded route produces `die.wav`, visibly decrements the life counter, and resets to `READY!`; a follow-up maps all 20 WAV sources and dispatches 18 observed events through the headed sink | Normal-path Stage 1, one collision/life cycle, and headed WAV dispatch verified; persistence, full-content, physical-mixer, and long-run play remain open | Verify repeated collision/life cycles, persistence, full content, and long-run play |
| `1500C` | The Sims Bowling | Focused 2026-08-27 release checkpoint: 700 guest frames, 39 hashes/changes, up to two draws, two zero-draw rows, and no fatal signatures; the default path decodes `GL_TEXTURE_RECTANGLE`/`GL_PALETTE8_RGBA8_OES` and shows a small legible `The` follow-up element, but menu/gameplay is not reached | Title + partial follow-up | Decode the `gameLib.rlb`/scene handoff, then exercise bowling controls |
| `1500E` | The Sims Pool | Focused 2026-08-27 release checkpoint: 700 guest frames, 31 hashes/changes, up to two draws, one zero-draw row, and no fatal signatures; the default path shows a small colored `The` follow-up element through the ordinary `297x75` atlas, but menu/gameplay is not reached | Title + partial follow-up | Decode the `gameLib.rlb`/scene handoff, then exercise aim/power/spin |
| `1B200` | LOST | 700 frames, zero GL draws, blank framebuffer; rserver loads but no shader output | Shader/render-server blocked | Parse or emulate the `rserver.bin` programmable path |
| `1C300` | musika | One captured frame with the Musika logo; no fatal | Splash only | Reverse the animation/packet/sound-bank runtime after the first scene |
| `33333` | Texas Hold'em | Corrected run completes 30,000,000 cycles / 700 captured frames with 3 visual hashes, 34 draws per steady-state frame, and no fatal; the stabilized capture is the `LOADING` screen | Loading screen only; playable state not reached | Reverse the title-specific resource completion/transition path, then verify poker-table controls and sound |
| `44444` | Zuma | Deterministic entry reaches the Temple, all built-in instruction pages, tutorial close, and live `LEVEL 1-1: SPIRAL OF DOOM`; commit `b12cd60` renders the spiral path and colored marbles coherently, and a durable 400M-cycle probe shows repeated post-entry fire/result activity including `+80 SLOWDOWN BALL`, with no fatal signature; the title-specific audio probe maps 36 sources and resolves 24 live events to named WAVs | Repeated fire/result activity and headless audio source/event routing verified; aim-angle parity, headed mixer behavior, and save remain open | Repeat controlled wheel-to-aim/fire sequences, then verify headed audio and persistence |
| `50513` | Sudoku | Reversed-register event heads drain cleanly; an opt-in RLB completion honors the late resource seek/read path and reaches coherent `PLAYER NAME`, `GAME SETUP`, `TUTORIAL`, and populated 9×9 puzzle-board scenes. The board cursor and numbered palette render; the default path remains unchanged | Puzzle-board/input partial under title-scoped gates; legal cell entry, audio, and save parity not verified | Calibrate board cursor/number controls, then verify error checking, audio, and persistence |
| `50514` | Royal Solitaire | Default path is a coherent character splash with consumed scripted events and a completed savefile request. Opt-in completion stages the full `Solitaire.rlb`; the owner-payload probe reaches the guest's first 4,096-byte RLB read, but the longer request cycle still does not reach the card board | Coherent splash; RLB payload path and first guest read proven diagnostically; post-RLB request contract unresolved | Reconstruct the post-RLB stream/request lifecycle, then validate card/selection interaction |
| `55555` | Bejeweled | Normalized wheel input reaches the tutorial and live 8×8 board; the deterministic replay enters the guest swap path, changes/refills tiles, and opens the score-bar overlay; the current-tree replay maps the known sources and emits `swap.wav`, `gotset.wav`, `gemongem.wav`, and `combo2.wav`; the headed sink accepts the mapped events; mode/save/mixer coverage remains | Single live match verified; guest WAV routing and headed sink dispatch partial; mode/save/mixer coverage remains | Extend the current valid-match replay to combo/excellent coverage, then verify persistence and mixer behavior |
| `66666` | Tetris | Fresh controlled evidence proves the board, pause/resume, opposite side-button rotations, signed wheel movement, hard drop, a guest row clear, indexed `Menu.wav`/`Move.wav`/`Drop.wav`/`Lock.wav`/`Clear.wav` events, headed sink playback including `Clear.wav`, and clean wall-adjacent rotations at both outer columns | Best current target; core gameplay/audio path verified, interactive partial, not complete | Prove collision-dependent kick behavior, then finish piece sequencing/scoring, persistence, long-run visual parity, and physical mixer parity |
| `77777` | Mahjong | 700 frames, 71 hashes, up to eight draws; mostly dotted/garbled title output | Texture/UV partial | Decode the `main.rlb` resource path and tile atlas |
| `88888` | Mini Golf | 700 frames, two hashes, five draws; mostly black with a loading/progress outline | Splash/loading only | Load the compact course resources and reach the menu |
| `99999` | Cubis 2 | 700 frames, 26 hashes and 27 changes, up to 49 draws; black/loading state despite many staged assets | Asset/renderer blocked | Decode the `.raw`/`.pix` image path and material handles |
| `AAAAA` | PAC-MAN | Normal path reaches name entry, the guest informational prompt, main menu, and the `START GAME / MODE / STAGE / BACK` screen, then reaches a rendered Stage 1 maze, moves Pac-Man, advances the score 0→30→40 through frame 1048 with no fatal signature, opens the guest `PAUSE` menu, resumes to the live maze after a settled Select edge, and the headed replay records 12 mapped WAV events accepted by the sink; no request for the executable's missing `tex_menu.tga` | Normal-path Stage 1, pause/resume, and headed WAV dispatch verified; physical mixer parity, collisions/lives, exit/save, persistence, and long-run play remain open | Verify collision/life cycles, exit/save, physical audio/mixer behavior, persistence, full content, and long-run play |

No row is marked fully playable. Tetris now has a guest-driven line-clear and
headed audio receipt, and Ms. PAC-MAN and PAC-MAN have normal-path maze/input
results; all remain incomplete under the project goal. The Ms. PAC-MAN details are in the
[gameplay probe](20260827_mspacman_gameplay_probe.md).
The PAC-MAN follow-up is in the
[PAC-MAN gameplay probe](20260828_pacman_gameplay_probe.md).

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
  confirms coherent title screens and the shared post-title draw family.
- OpenGLES:19 now recognizes the Sims rectangle-texture target (`0x84f5`) for
  `GL_PALETTE8_RGBA8_OES` uploads. Bowling's default post-title path now
  decodes its `354x25` text atlas; Pool's ordinary atlas path remains stable.
- A title-scoped `FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE=1` probe stages each Sims
  `gameLib.rlb` and completes its guest callback chain, but still produces no
  menu transition. It remains diagnostic-only and is excluded from the default
  matrix.
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
- The desktop host sink is now headed-verified for Tetris's indexed menu,
  movement, drop, lock, and clear WAV events through the persistent `rodio`
  sink. Physical output, overlap, timing, volume, and mixer parity still need
  a headed waveform regression against an iPod/reference recording.
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
- The durable Zuma gameplay probe now reaches beyond board entry: repeated
  wheel rotations followed by Select produce a visible projectile and a later
  `+80 SLOWDOWN BALL` collision/result bonus. This verifies repeated
  input-to-gameplay activity, while controlled aim-angle parity, audio, and
  persistence remain unverified. See the [Zuma gameplay probe](20260827_zuma_gameplay_probe.md).
- Zuma's source-registration and trigger sequence is now correlated from
  `Audio:0` through the WAV header read and final `Audio:2` commit. A focused
  replay maps 36 sources and resolves 24 live events to named WAV paths in the
  headless queue; headed sink playback, mixer parity, aim-angle control, and
  persistence remain open. See the [Zuma audio ABI probe](20260827_zuma_audio_abi.md).
- Executable discovery ignores archive-generated AppleDouble `._*.bin`
  sidecars. The durable corpus extracted from the external drive contains
  those metadata files alongside the real plaintext eApp binaries, so this is
  covered by every subsequent corpus run.
- The legacy `Filesytem` ABI now has independent synthetic handles,
  sequential host-backed reads, and close semantics. Bejeweled reaches its
  menu, 8×8 board, and “Selecting Gems” tutorial; normalized wheel input now
  drives a deterministic live-board match with visible refill and score-bar
  results. Bejeweled now has title-specific WAV source/event routing and
  headed sink dispatch, while match-specific audio, physical mixer parity, and
  save checks remain open.
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
  accepted. A follow-up staged the RLB and reached both parser reads, but a
  direct linked-callback experiment faulted at `0x18022f80`; the temporary
  completion code was removed. See the [readiness-contract probe](20260826_royal_readiness.md)
  and [RLB callback probe](20260827_royal_rlb_callback_probe.md).
- Sudoku's title-scoped RLB probe now applies the guest's absolute seek before
  the late 153,884-byte resource read. Corrected centered half-texel UV
  containment restores the two previously skipped setup-panel draws, producing
  coherent `PLAYER NAME` and `GAME SETUP` captures. Completing the name and
  dismissing the tutorial now reaches a populated 9×9 puzzle board with its
  cursor and numbered palette. The probe remains opt-in; legal cell entry,
  audio, and persistence parity are not claimed. See the [Sudoku RLB/setup
  receipt](20260827_sudoku_rlb_seek_and_setup.md) and [puzzle-board/input
  receipt](20260827_sudoku_puzzle_board_and_input.md).

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
