# Decrypted-game HLE baseline

This is the starting compatibility record for `fliwheel`.

Source: clicky commit `d1d735973f404ca53cd3e4b9f6e4e3dcb38b4df1`  
Runner: `target/release/eapp`  
Mode: headless experimental GL HLE, approximately eight seconds per bundle  
Input: no wheel or button events injected  
Timeout exit `124`: expected watchdog termination, not a crash

## Core 16 bundles

| State | Bundles |
| --- | --- |
| Stable startup/idle/attract rendering | Tetris (`66666`), Cubis 2 (`99999`), Texas Hold'em (`33333`), Ms. Pac-Man (`14004`), Pac-Man (`AAAAA`), Mahjong (`77777`), Mini Golf (`88888`), Sims Bowling (`1500C`), Sims Pool (`1500E`) |
| Splash/idle only | Sudoku (`50513`) |
| Content followed by a DMA wait | Bejeweled (`55555`), Zuma (`44444`) |
| Partial rendering | Vortex (`12345`), Royal Solitaire (`50514`) |
| Blocked | TWA/iQuiz (`11002`), Lost (`1B200`) |

“Stable” means the HLE reaches a repeating render loop and produces content;
it does not yet claim pixel parity or complete interactive gameplay. The two
DMA cases stop when the game waits for hardware completion. Vortex is limited
by pointer/VBO indirection, Solitaire by texture association, TWA by pack/file
loading and texture association, and Lost by its render-server path.

Four additional decrypted bundles were also smoke-tested: SAT Prep Reading,
Writing, Mathematics, and musika reach title/loading content and then stall.

## Reproduction

Use the repository script with an external `Games_RO` directory:

```sh
TIMEOUT_SECONDS=8 ./scripts/test_decrypted_games.sh /path/to/Games_RO
```

The script writes timestamped reports and per-game logs under the ignored
`docs/game_tests/` run-log directory when `REPORT_DIR` is set. It prefers
`gtimeout` on macOS and `timeout` elsewhere.

