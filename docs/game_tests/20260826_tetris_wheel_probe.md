# Tetris wheel movement probe

Date: 2026-08-26

This is the isolated gameplay-wheel follow-up for bundle `66666`. It extends
the broader control receipt in
[`20260826_tetris_gameplay_controls.md`](20260826_tetris_gameplay_controls.md)
with a visual assertion that signed wheel packets move the active piece.

## Run

The probe used the real bundle-id directory name so the title-specific Tetris
runtime path was selected:

```text
Bundle: /tmp/fliwheel_tetris_local_games.KfoVlx/Games_RO/66666
Capture: /tmp/fliwheel_tetris_wheel_centered.eBQE6w
Frames: guest 0..379
Cycles: 50,000,000
Input:
  action:15-16,wheel=37:30-31,action:45-46,action:78-79,
  action:100-101,action:150-151,action:210-211,
  wheel=4:240-245,wheel=-4:270-275,down:330-331
```

The original bundle was read from the durable external corpus, but that
external-volume run stalled in the emulator's asynchronous file-import path
after only two frames. The 17 MiB bundle was copied to the local temporary
directory above; the result below is therefore a local-host timing probe, not
a claim about external-volume I/O performance. The run explicitly enabled the
title-scoped parsed-resource completion path used by the other current Tetris
receipts.

## Result

- Exit status: `0`; no fatal signature, panic, or process error.
- Capture: 380 guest rows, frames `0..379`, 380 PPMs.
- Presented framebuffer: 54 unique hashes and 53 sequential hash changes.
- Draws: 382 at the initial resource-heavy frame, then a stable 16-draw
  gameplay scene; two startup rows had zero draws.
- The input trace records `wheel_delta: 4.0` at frames 240-245 and
  `wheel_delta: -4.0` at frames 270-275.
- The board remains visually centered during the wheel assertion. The later
  `down` transition at frame 330 is deliberately outside the wheel boundary;
  its 47-draw transition remains a separate renderer/parity gate.

The dominant red active-piece component gives the visual assertion:

| Boundary | Wheel input | Red active-piece bounds | Observation |
|---:|---:|---:|---|
| 240 | `+4` begins | x=149..168, y=12..22 | Centered-board baseline before the visible sweep |
| 245 | `+4` held | x=193..212, y=12..22 | Piece moved right while the board stayed centered |
| 270 | `-4` begins | x=182..212, y=12..33 | Signed reverse sweep starts from the right position |
| 275 | `-4` held | x=127..157, y=12..33 | Piece moved back left |

The x change is the wheel response. The y/height difference at the reverse
boundary reflects the active-piece draw state and not a board-origin change.
A single-frame `wheel=1` packet in the earlier control run was not sufficient
to make this assertion, which is why this receipt uses sustained signed
sweeps.

## Boundary

Tetris now has evidence for menu/name-entry wheel navigation and gameplay
horizontal movement, alongside pause/resume, gravity, side-button response,
and hard drop. Exact physical scaling and labels, rotation behavior, line
clear/game-over parity, persistence, long-run visual comparison, and host
audio mixing remain open.
