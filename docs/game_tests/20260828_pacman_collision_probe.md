# PAC-MAN collision/life probe

Date: 2026-08-28 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Result

The normal Stage 1 path now has one verified collision/life cycle. The probe
uses the same measured startup route as the ordinary gameplay test, then drives
a repeatable sequence of quadrant packets around the maze:

```text
0x40000030:800-899
0x40000070:900-1099
0x400000b0:1100-1299
0x400000f0:1300-1499
```

The fresh bounded replay shows:

1. The post-start maze has three visible life icons at frame 1400.
2. The guest queues `die.wav` at frame 1500:
   `AudioEvent frame=1500 type=1 index=2 path=.../audio/die.wav`.
3. The death transition clears the active maze sprites and the guest returns to
   `READY!` by frame 1550.
4. The visible life count is two from frame 1548 onward, confirming one life
   was consumed rather than treating a sprite disappearance as a collision.

The run reaches the bounded frame 1600 with no fatal, fault, panic, decoder, or
sink error. This proves one collision/death/reset path and the corresponding
named audio event. Repeated life cycles, game-over, exit/save, persistence,
physical mixer behavior, and long-run play remain open.

## Reproduction

Build the release runner, then use the decrypted bundle:

```bash
cargo build --release --bin eapp

env \
  EAPP_AUDIO_EVENT_TRACE=1 \
  FLIWHEEL_AUDIO_TRACE=1 \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=0 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_TRACE=1 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_INTERVAL=200 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:100-252,action:260-265,action:300-305,action:350-355,action:750-755,bits=0x40000030:800-899,bits=0x40000070:900-1099,bits=0x400000b0:1100-1299,bits=0x400000f0:1300-1499,bits=0x40000030:1500-1699,bits=0x40000070:1700-1899,bits=0x400000b0:1900-2199' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_pacman_collision_proof_20260828 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
  FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=1400 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=1600 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=250 \
  FLIWHEEL_EAPP_STOP_FRAME=1600 \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA' \
  --headless --cycles 1100000000
```

The late capture begins after the initial life-icon normalization so the
collision transition can be measured independently of the normal `READY!`
startup state.

## Evidence

- Log: `/tmp/fliwheel_pacman_collision_proof_20260828.log`
- Capture manifest: `/tmp/fliwheel_pacman_collision_proof_20260828/manifest.tsv`
- Frame 1500: `/tmp/pac_proof_1500.png`, death-transition window with the
  `die.wav` event at the same guest frame.
- Frame 1550: `/tmp/pac_proof_1550.png`, guest-rendered `READY!` reset with two
  visible life icons.
- The manifest contains one row per frame from 1400 through 1599; the life
  icon transition is detected at frame 1548.

The broader normal maze, pause/resume, and headed audio results remain in
[`20260828_pacman_gameplay_probe.md`](20260828_pacman_gameplay_probe.md).
