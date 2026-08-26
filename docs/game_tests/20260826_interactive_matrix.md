# Decrypted clickwheel-game interactive matrix

Date: 2026-08-26 UTC  
Repository: `fliwheel`  
Corpus: `/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO`  
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
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO'
```

The earlier family reports are in `/tmp/fliwheel_interactive_full_{a,b,c,d}`;
they predate the title-scoped completion correction. The canonical corrected
report is `/tmp/fliwheel_interactive_corrected_20260826/interactive_matrix.md`.
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
| `44444` | Zuma | 7 frames, 2 hashes, up to eight draws; only a tiny top-of-screen fragment is visible | Texture/transform partial | Verify atlas/RO resource upload and PopCap board composition |
| `50513` | Sudoku | Recognizable Sudoku title screen; 700 frames but 699 zero-draw idle frames after input | Splash/menu reached; content input unverified | Derive the start/menu event contract and render the puzzle grid |
| `50514` | Royal Solitaire | 700 frames, 26 hashes and 62 changes; recognizable character art, but large UV/quad corruption and 635 zero-draw frames | Partial splash/menu; not playable | Fix the shared UV/atlas selection and validate card interaction |
| `55555` | Bejeweled | 6 frames, 3 hashes and 3 changes, up to 37 draws; title/jewel fragments but no coherent board | PopCap partial | Repair DMA/texture composition and then test wheel selection/moves |
| `66666` | Tetris | Corrected run reaches frame 501 with 20 hashes and up to 382 draws; the separate targeted schedule reaches the board, pause/resume, left/right, hard drop, and indexed `Menu.wav`/`Move.wav`/`Drop.wav` events | Best current target; interactive partial, not complete | Finish wheel displacement, line clears, persistence, long-run visual parity, and sound mixing |
| `77777` | Mahjong | 700 frames, 71 hashes, up to eight draws; mostly dotted/garbled title output | Texture/UV partial | Decode the `main.rlb` resource path and tile atlas |
| `88888` | Mini Golf | 700 frames, two hashes, five draws; mostly black with a loading/progress outline | Splash/loading only | Load the compact course resources and reach the menu |
| `99999` | Cubis 2 | 700 frames, 26 hashes and 27 changes, up to 49 draws; black/loading state despite many staged assets | Asset/renderer blocked | Decode the `.raw`/`.pix` image path and material handles |
| `AAAAA` | PAC-MAN | 425 frames, two hashes, up to 58 draws; recognizable Namco loading screen, content not reached | Loading screen only | Advance through menu and verify maze/D-pad movement |

No row is marked fully playable. The only title with content-level control
evidence is Tetris, and even that remains incomplete under the project goal.

## Shared changes made after this run

- The NDC detector now includes the empirically matching Sims Bowling/Pool
  family (`1500C`/`1500E`) in addition to Sudoku/Solitaire. A targeted rerun
  confirms the titles remain stable, but does not yet show useful coverage.
- The resource-indexed audio observer is now scoped to Tetris (`66666`). The
  first corpus run showed that the same virtual addresses are unrelated code
  in other binaries and produced fabricated event IDs; those counts are not
  valid audio evidence and are excluded from this matrix.
- `CLICKY_EAPP_ASYNC3_COMPLETE=1` is now title-scoped in both the runtime and
  interactive harness. It is a provisional parsed-resource experiment for
  Tetris only; applying its completion fields to Hold'em reaches a different
  resource ABI and produced a misleading null-table fault.
- The desktop host sink remains headless-unverified. Tetris has a persistent
  `rodio` sink and a guest-indexed event queue, but physical output, overlap,
  timing, and mixer parity still need a headed test and waveform regression.

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
