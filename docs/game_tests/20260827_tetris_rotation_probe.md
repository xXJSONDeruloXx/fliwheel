# Tetris rotation and control probe

Date: 2026-08-27  
Bundle: `66666` (`Tetris_1_1_2563292.bin`)

This probe follows the centered-board and hard-drop receipts. It combines a
fresh 700-frame capture with the bundle's static `InputEvents` event-list
handler so that a draw change is not being mistaken for a host-only input
change.

## Reproduce

The run used the durable decrypted bundle and the parsed Tetris resource path:

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,left:300-301,right:330-331,wheel=4:360-365,down:400-401' \
FLIWHEEL_STARTUP_PROGRESS_TRACE=1 \
FLIWHEEL_STARTUP_PROGRESS_INTERVAL=50 \
FLIWHEEL_STARTUP_PROGRESS_FRAMES=700 \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_tetris_next_20260827.atpdNd/captures \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=700 \
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=700 \
RUST_LOG='EAPP_PROGRESS=info,EAPP_INPUT=info,EAPP_GL=info,EAPP=warn,EAPP_IMPORT=info' \
target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/66666' \
  --headless --cycles 50000000
```

The capture manifest has 700 frame rows. The run log is
`/tmp/fliwheel_tetris_next_20260827.atpdNd/run.log`; the PPM files are in the
capture directory above. The run completed without a fatal guest memory
error, panic, unsupported-upload fatal, or emulator process error.

## Observed controls

The dominant red active-piece component was measured from the captured
framebuffer. Gravity accounts for the small vertical changes between some
rows; the orientation and horizontal changes below occur at the input edges.

| Guest frame | Input | Red active-piece observation | Result |
|---:|---|---|---|
| 299 | none | `x=138..168`, `y=24..43` | Baseline piece |
| 300 | `left` press | `x=149..168`, `y=25..54` | Piece orientation changes |
| 301 | held/released path | `x=149..168`, `y=36..65` | Same rotated orientation while falling |
| 329 | none | `x=149..168`, `y=36..65` | Pre-rotation comparison |
| 330 | `right` press | `x=138..168`, `y=35..54` | Opposite orientation change |
| 359 | none | `x=138..168`, `y=46..65` | Wheel baseline |
| 360..365 | `wheel=4` | x origin advances from `138` to `182` | Active piece moves right; board stays fixed |
| 399 | none | `x=182..212`, `y=57..76` | Pre-drop |
| 400 | `down` press | red piece relocates to the floor; draw count jumps `16` → `47` | Hard drop/lock transition |

The static per-frame function at `0x180222a4` passes the compact event list to
the linked handler at `0x180055dc`. Its press mapping is:

| Event id | Guest button bit | fliwheel packet | Guest behavior |
|---:|---:|---|---|
| 1 | `0x10` | `menu` | Menu/back |
| 2 | `0x01` | `action` | Select/pause/resume |
| 3 | `0x02` | `left` | One rotation direction |
| 4 | `0x04` | `right` | Opposite rotation direction |
| 5 | `0x08` | `up` / `down` | Vertical/drop action; `down` is the observed hard drop |

This establishes the core control family seen in the contemporary [iLounge
Tetris review](https://www.ilounge.com/index.php/reviews/entry/electronic-arts-tetris):
wheel movement, side-button rotation, and center/down dropping. The visual
probe proves opposite rotation steps, but the names clockwise and
counter-clockwise remain display-orientation dependent until checked against a
physical device or reference video frame-by-frame.

## Boundary

Tetris now has evidence-backed board entry, pause/resume, gravity, horizontal
wheel movement, opposite side-button rotations, and hard drop. This is still
not a full gameplay-parity result: rotation wall/kick behavior, line clears,
piece sequencing, persistence, long-run visual parity, and physical audio
mixing remain open.
