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
Capture: /tmp/fliwheel_tetris_wheel_probe.SW9wM7
Frames: guest 0..499
Cycles: 50,000,000
Input:
  action:15-16,wheel=37:30-31,action:45-46,action:78-79,
  action:100-101,action:150-151,action:210-211,action:230-231,
  action:260-261,wheel=4:300-305,wheel=-4:330-335,down:400-401
```

The original bundle was read from the durable external corpus, but that
external-volume run stalled in the emulator's asynchronous file-import path
after only two frames. The 17 MiB bundle was copied to the local temporary
directory above; the result below is therefore a local-host timing probe, not
a claim about external-volume I/O performance.

## Result

- Exit status: `0`; no fatal signature, panic, or process error.
- Capture: 500 guest rows, frames `0..499`, 500 PPMs.
- Presented framebuffer: 58 unique hashes and 57 sequential hash changes.
- Draws: 382 at the initial resource-heavy frame, then a stable 16-draw
  gameplay scene; two startup rows had zero draws.
- The input trace records `wheel_delta: 4.0` at frames 300-305 and
  `wheel_delta: -4.0` at frames 330-335.

The dominant red active-piece component gives the visual assertion:

| Boundary | Wheel input | Red active-piece bounds | Observation |
|---:|---:|---:|---|
| 300 | `+4` begins | x=138..168, y=34..55 | Baseline before the visible sweep |
| 305 | `+4` held | x=182..212, y=45..66 | Piece moved right while gravity advanced |
| 330 | `-4` begins | x=182..212, y=45..66 | Signed reverse sweep starts from the right position |
| 335 | `-4` held | x=127..157, y=45..66 | Piece moved back left |

The y change is consistent with the concurrent falling animation; the x
change is the wheel response. A single-frame `wheel=1` packet in the earlier
control run was not sufficient to make this assertion, which is why this
receipt uses sustained signed sweeps.

## Boundary

Tetris now has evidence for menu/name-entry wheel navigation and gameplay
horizontal movement, alongside pause/resume, gravity, side-button response,
and hard drop. Exact physical scaling and labels, rotation behavior, line
clear/game-over parity, persistence, long-run visual comparison, and host
audio mixing remain open.
