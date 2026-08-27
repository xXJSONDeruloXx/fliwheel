# Tetris line-clear and headed audio probe

Date: 2026-08-27
Bundle: `66666` (`Tetris_1_1_2563292.bin`)

This probe closes the earlier line-clear test boundary with a genuine guest
placement sequence. The HLE supplies only the input packets, resource reads,
and normal audio sink; it does not synthesize a clear or alter the guest board.

## Reproduce

The controlled sequence enters gameplay, places Z/O/S/L, then rotates and
drops T. The four-frame L wheel window is intentional: it lands that piece at
guest x=3, completing the row implied by the preceding board masks.

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,wheel=-3:300-305,down:330-331,wheel=-4:360-370,down:390-391,wheel=1:420-425,down:450-451,wheel=-1:480-483,down:510-511,wheel=4:540-545,left:550-551,left:555-556,down:570-571' \
EAPP_AUDIO_EVENT_TRACE=1 \
EAPP_AUDIO_DISABLE=1 \
FLIWHEEL_EAPP_PC_TRACE='0x1800b6e8,0x18008ef4,0x18020b30' \
FLIWHEEL_EAPP_PC_TRACE_LIMIT=12 \
FLIWHEEL_EAPP_PC_TRACE_DETAIL=1 \
RUST_LOG='EAPP_AUDIO=info,EAPP_PC_TRACE=info,EAPP_INPUT=warn,EAPP_GL=warn,EAPP=warn,EAPP_IMPORT=warn' \
target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/66666' \
  --headless --cycles 70000000
```

The headless event trace is retained at
`/tmp/fliwheel_tetris_line_clear_20260827.log`. The headed, audio-enabled
replay is at `/tmp/fliwheel_tetris_line_clear_headed_20260827.log`.

## Evidence

The active-piece/board dump recorded these guest states immediately before
each hard drop:

| Guest frame | Piece | Guest x | Board result |
|---:|---|---:|---|
| 330 | Z (`3`) | 1 | first placement |
| 390 | O (`5`) | 0 | bottom-left cells present |
| 450 | S (`2`) | 5 | right-side cells present |
| 510 | L (`1`) | 3 | row-completing geometry |
| 570 | T (`6`) | 8 | rotated drop triggers clear |

At frame 570 the guest emitted both `Drop.wav` and `Clear.wav`. At frame 576
the traced guest calls include `0x18020b30` and the row-clear routine at
`0x18008ef4`. The headed run independently logged `played sound` for
`Clear.wav` through the persistent host sink, alongside the normal movement,
drop, and lock sounds. The run exited successfully with no fatal guest-memory,
panic, or emulator error signature.

## Boundary

This verifies a real line clear from guest state through guest audio event and
headed host playback. It does not establish full Tetris parity: wall/kick
rules, piece randomization, scoring/level timing, save/load, long-run visual
comparison, and physical mixer timing remain open. Tetris is therefore the
closest title, but not perfect or near-perfect.
