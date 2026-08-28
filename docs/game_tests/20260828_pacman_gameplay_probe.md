# PAC-MAN diagnostic gameplay probe

Date: 2026-08-28 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Result

The title-scoped PAC-MAN TGA completion path now gets past the previous
pre-maze null-pointer fault. The guest's completion trampoline reads the
request status and byte count at `[request+0x20]` and `[request+0x24]`; the
HLE supplies those fields for the two PAC-MAN `.tga` reads. The next guest
frame passes the byte count into the resource manager, which allows the
guest's own TGA parser to populate the texture dimensions.

The route reaches the first maze and remains stable through the bounded probe:

1. Frame 775 enters the maze scene with 33 draws per frame.
2. Frame 800 shows the rendered maze in `READY` state with score `0`.
3. The scripted clickwheel quadrant inputs move Pac-Man and animate the
   ghosts; frame 880 shows score `30` and frame 890 shows score `40`.
4. The run continues through frame 1048 with changing maze frames, draw counts
   of 32-37, and no fatal signature.

This is the first PAC-MAN evidence of actual in-maze control in fliwheel. It
is diagnostic-only: the completion flag is not enabled by the normal launcher,
and this run disabled audio output. Collision/life behavior, pause/exit,
audible playback, persistence, and long-run play remain open.

## Reproduction

Build the release runner, then use the decrypted bundle with the PAC-MAN-only
completion flag:

```bash
cargo build --release --bin eapp

env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_AUDIO_TRACE=1 \
  FLIWHEEL_EAPP_PACMAN_ASYNC3_COMPLETE=1 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_TRACE=1 \
  FLIWHEEL_EAPP_STARTUP_PROGRESS_INTERVAL=25 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:100-252,action:260-265,action:300-305,action:350-355,action:750-755,bits=0x400000f0:800-830,bits=0x40000030:850-880,bits=0x40000070:900-930,bits=0x400000b0:950-980' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_pacman_movement_20260828 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=10 \
  FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=780 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=1050 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=40 \
  FLIWHEEL_EAPP_STOP_FRAME=1050 \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA' \
  --headless --cycles 650000000
```

The quadrant values used here are `0xf0`, `0x30`, `0x70`, and `0xb0`, each
with the clickwheel-touch bit `0x40000000`. Menu/action edges select the name,
confirm it, enter Play Game, and start Stage 1.

## Evidence

- Log: `/tmp/fliwheel_pacman_movement_20260828.log`
- Capture manifest: `/tmp/fliwheel_pacman_movement_20260828/manifest.tsv`
- Visual captures include frames 800, 840, 880, and 890 in
  `/tmp/fliwheel_pacman_movement_20260828/`.
- The 880 and 890 captures visibly show the score changing from `30` to `40`
  while the maze and sprites remain coherent.
- Audio imports/events for PAC-MAN's WAV set were observed in the log, but
  `FLIWHEEL_EAPP_AUDIO_DISABLE=1` means this probe does not prove audible
  playback.

## Default-path boundary

With `FLIWHEEL_EAPP_PACMAN_ASYNC3_COMPLETE` unset, the prior path remains
unchanged: the start route reaches guest initialization states 2-9 and then
faults at `PC 0x1801628c` before the maze. The next implementation step is to
validate the opt-in callback contract across the normal launcher and then make
it the default title-specific behavior once audio and startup regressions are
covered.
