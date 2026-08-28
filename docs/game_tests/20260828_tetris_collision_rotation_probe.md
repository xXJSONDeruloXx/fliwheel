# Tetris collision-dependent rotation probe

Date: 2026-08-28  
Bundle: `66666` (`Tetris_1_1_2563292.bin`)

This follow-up builds a settled stack with the guest's own hard-drop path and
then rotates the next active piece at collision height. It distinguishes an
ordinary edge rotation from a rotation rejected by occupied board cells.

## Reproduce

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,wheel=-3:300-305,down:330-331,wheel=-4:360-370,down:390-391,wheel=1:420-425,down:450-451,wheel=-1:480-483,down:510-511,wheel=4:540-545,left:550-551,left:555-556,down:570-571,down:650-651,down:750-751,down:850-851,down:950-951,down:1050-1051,left:1500-1501,right:1530-1531,left:1560-1561,right:1590-1591,down:1650-1651' \
EAPP_PC_TRACE='0x1800b6e8,0x1800bdf0,0x1800b7d0,0x1800b4f8,0x1800b574,0x1800a060,0x1800b6b8,0x18008ef4,0x18020b30' \
EAPP_PC_TRACE_LIMIT=100 \
EAPP_PC_TRACE_DETAIL=1 \
RUST_LOG='EAPP_PC_TRACE=info,EAPP_INPUT=warn,EAPP_GL=warn,EAPP_AUDIO=warn,EAPP=warn,EAPP_IMPORT=warn' \
target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/66666' \
  --headless --cycles 160000000
```

The raw trace is retained at
`/tmp/fliwheel_tetris_collision_rotation_attempt_20260828.log`.

## Evidence

- Repeated guest hard drops build a non-empty stack; the board masks at the
  bottom rows grow across the drops, with no host-side board synthesis.
- At guest frame 1500, a type-1 piece at approximately `x=4, y=30, rot=0`
  enters the rotation path `0x1800b7d0 -> 0x1800b4f8 -> 0x1800b574 ->
  0x1800a060` and reaches `0x1800b6b8`, the failure branch after the
  collision check. The occupied stack is present in the same trace record.
- At frame 1530, the opposite rotation path
  `0x1800bdf0 -> 0x1800b4f8 -> 0x1800b574 -> 0x1800a060` completes without
  the failure marker, demonstrating an accepted rotation in the same built-up
  board state.
- The alternating probes at frames 1560 and 1590 reproduce the same rejected
  and accepted split. The final hard drop at frame 1650 locks the piece and
  the run completes without a fatal guest-memory, panic, or emulator error
  signature.

This establishes collision-dependent blocked rotations against settled cells,
in addition to the earlier wall-adjacent rotation smoke. It does not yet
prove a kick translation: the trace has not shown a rejected candidate being
retried at a translated x/y coordinate. Formal kick tables, piece sequencing,
scoring/level timing, persistence, long-run visual comparison, and physical
mixer timing remain open.
