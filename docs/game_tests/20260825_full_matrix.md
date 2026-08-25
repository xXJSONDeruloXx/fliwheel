# Full decrypted-game matrix

Date: 2026-08-25

This is the post-change 20-bundle smoke run for the decrypted corpus. It is a
startup/rendering regression, not a claim that every title is fully playable.

Corpus: `/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO`

Runner: `/Users/danhimebauch/Developer/fliwheel/target/release/eapp`

Mode: headless experimental GL HLE, 8-second watchdog per bundle, no input

Raw report: `/tmp/fliwheel_regression_20260825/20260825_190150_decrypted_games.md`

Per-game logs: `/tmp/fliwheel_regression_20260825/20260825_190150_logs`

| Bundle | Exit | Last frame | Last draws | Rasterized draws | Skipped draws |
| --- | ---: | ---: | ---: | ---: | ---: |
| 11002 | 124 | 5606 | 1 | 0 | 5608 |
| 11050 | 124 | 3 | 2 | 7 | 0 |
| 11051 | 124 | 3 | 2 | 7 | 0 |
| 11052 | 124 | 3 | 2 | 7 | 0 |
| 12345 | 124 | 4799 | 2 | 10124 | 0 |
| 14004 | 124 | 1874 | 12 | 18740 | 1 |
| 1500C | 124 | 1766 | 1 | 1732 | 0 |
| 1500E | 124 | 2376 | 1 | 2376 | 0 |
| 1B200 | 124 | 5052 | 0 | 0 | 0 |
| 1C300 | 124 | 0 | 1 | 1 | 0 |
| 33333 | 124 | 673 | 34 | 22551 | 0 |
| 44444 | 124 | 6 | 8 | 35 | 1 |
| 50513 | 124 | 3349 | 0 | 1 | 0 |
| 50514 | 124 | 1136 | 0 | 233 | 3 |
| 55555 | 124 | 5 | 37 | 144 | 1 |
| 66666 | 124 | 1061 | 29 | 7387 | 0 |
| 77777 | 124 | 2007 | 9 | 14926 | 1 |
| 88888 | 124 | 2707 | 5 | 8121 | 2 |
| 99999 | 124 | 634 | 49 | 28086 | 0 |
| AAAAA | 124 | 1365 | 14 | 18189 | 3 |

All 20 exits were the expected watchdog `124`. A scan of the per-game logs
found no fatal memory fault, panic, or emulator crash. The draw totals vary
with host timing, but the corpus shape remains consistent with the starting
baseline: 11002 pack-load blocked, 1B200 shader blocked, 12345 VBO-limited,
50514 partially associated, 44444/55555 DMA-boundary cases, and 50513 in its
NDC input-wait loop without injected input.
