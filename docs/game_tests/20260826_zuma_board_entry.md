# Zuma live-board entry receipt

Date: 2026-08-26  
Bundle: `44444`  
Purpose: drive the verified title/tutorial path through the first live Zuma
board and separate input progress from renderer completeness.

## Result

The HLE now reaches the live `LEVEL 1-1: SPIRAL OF DOOM` scene. The capture
contains the HUD, the centered frog, the level transition, a marble/firing
animation, and repeated Select edges after board entry. This is a real
gameplay-state receipt, but not a playable-parity receipt: the spiral path and
colored marbles are spatially scattered because the texture/material
association for the board's repeated `0x10` draws is still incomplete.

## Reproduction

Corpus executable:

```text
/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/44444/Executables/Zuma_1_1_2563298.bin
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

The full capture and log are retained at:

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
repeated board-object draws use material `0x10`, but the guest binds texture
name `0x8` while the captured uploads contain no corresponding `tex_name=0x8`
upload. The current generic fallback therefore chooses unrelated containing
uploads for many 17x17 UV slices. This explains the scattered path/ball
artifacts and is the next narrow renderer target.

## Remaining gates

Zuma still needs a faithful texture redefinition/bind lifetime model, coherent
spiral-track and marble composition, clickwheel rotation/aim behavior, firing
and collision verification, audio playback, and save/persistence coverage.

