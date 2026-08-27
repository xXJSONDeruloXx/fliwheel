# Zuma live-board entry receipt

Date: 2026-08-26; renderer update: 2026-08-27
Bundle: `44444`  
Purpose: drive the verified title/tutorial path through the first live Zuma
board and separate input progress from renderer completeness.

## Result

The HLE reaches the live `LEVEL 1-1: SPIRAL OF DOOM` scene. The original
board-entry capture contains the HUD, centered frog, level transition,
marble/firing animation, and repeated Select edges after board entry. The
texture-name fix in commit `b12cd60` now associates each PopCap upload with the
latest real `OpenGLES:4` bind, so the corrected replay renders the spiral path
and colored marbles coherently. A later durable-corpus control probe now also
shows post-entry Select-driven projectile and collision/result activity twice,
including a `+80 SLOWDOWN BALL` bonus; controlled aim-angle parity, full
playability, audio, and persistence are still open. See the [gameplay
probe](20260827_zuma_gameplay_probe.md).

## Reproduction

Corpus executable:

```text
/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/44444/Executables/Zuma_1_1_2563298.bin
```

Runner environment:

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
```

The deterministic input schedule first enters the Temple, advances through
the built-in tutorial, closes it, then presses Select twice on the live board:

```text
action:800-802, action:900-902, action:1000-1002,
action:1100-1102, action:1200-1202, action:1300-1302,
action:1400-1402, action:1500-1502, action:1600-1602,
action:1700-1702, action:1900-1902, action:2100-2102
```

The original full capture and log are retained at:

```text
/tmp/fliwheel_zuma_board_entry_20260826.95lN0i/capture/
/tmp/fliwheel_zuma_board_entry_20260826.95lN0i/run.log
```

## Observed milestones

| Guest frame | State | Evidence |
| ---: | --- | --- |
| `804` | Temple stage screen | `PRESS SELECT TO ENTER THE TEMPLE`, `STAGE 1`, `TEMPLE OF ZUKULKAN` |
| `1000`-`1602` | Built-in instructions | Five instruction pages and the final power-up legend render; Select advances each page |
| `1701`-`1703` | Tutorial close/loading transition | The final `CLOSE` edge is consumed and the resource-backed scene changes |
| `1704` | First live-board scene | HUD/frog scene begins with 61 draws |
| `1760` | Level title transition | `LEVEL 1-1` / `SPIRAL OF DOOM` appears with animated particles |
| `1880`-`2187` | Live gameplay loop | 52-79 draws per sampled frame, frog/marble animation, and Select edges at `1900` and `2100` |

The capture has 2,188 guest-frame rows, 503 presented hashes, and a 340-draw
peak during the title/tutorial transitions. The steady live-board samples
reach 79 draws. No fatal signature was observed.

## Renderer evidence

The live board's first full-surface draw uses material handle `0x16` and the
valid `322x222` RGBA4444 upload against the guest's `320x240` UV span. The
repeated board-object draws use material `0x10` and the guest binds texture
name `0x8`. The pre-fix bind trace at
`/tmp/fliwheel_zuma_bindtrace.NfTiSu/run.log` showed a stale pending `0x7`
surviving across that bind, tagging the `512x114` colored-marble atlas as
`tex_name=0x7`. Commit `b12cd60` makes the latest nonzero `OpenGLES:4` bind the
pending upload association, which removes that stale-name mismatch.

## Corrected renderer verification

The corrected capture at `/tmp/fliwheel_zuma_texturefix3.SDhEH4/` reached the
live board with 2,742 guest-frame rows, 588 presented hashes, and a 340-draw
peak. Frames 2500, 2600, 2700, and 2741 show a complete spiral track with
colored marbles distributed along it, the centered frog, HUD, and animated
next-ball state. The longer durable-corpus control replay at
`/tmp/fliwheel_zuma_shot.HNTruP/` completed 190,000,000 cycles and 7,600 guest
frames without a fatal signature; its tutorial close pulses were still
timer-misaligned, so it is not counted as a shot/collision result.

The same source build passed focused 30,000,000-cycle regressions for
Bejeweled (`55555`) and Tetris (`66666`) with exit `0` and zero fatal
signatures. Their report is at
`/tmp/fliwheel_bindfix_regression_20260826/interactive_matrix.md`.

The durable gameplay probe at
`/tmp/fliwheel_zuma_multishot.5FwyEK/` completed 400,000,000 cycles with exit
`0`, 7,851 guest-frame rows, 2,966 unique presented hashes, 3,337 actual hash
changes, and no fatal signature. Frames `7400`-`7420` show the first
post-entry projectile/result sequence; frames `7700`-`7710` show the later
`+80 SLOWDOWN BALL` result. The full receipt is [the Zuma gameplay
probe](20260827_zuma_gameplay_probe.md).

## Remaining gates

Zuma still needs repeatable clickwheel rotation/aim-angle parity, broader
several-shot collision-chain coverage, audio playback, and save/persistence
coverage.
