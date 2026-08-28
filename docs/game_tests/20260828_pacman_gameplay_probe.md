# PAC-MAN gameplay probe

Date: 2026-08-28 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Result

The normal PAC-MAN path now gets past the previous pre-maze null-pointer
fault. The guest's completion trampoline reads the request status and byte
count at `[request+0x20]` and `[request+0x24]`; the HLE supplies those fields
for the two PAC-MAN `.tga` reads. The next guest frame passes the byte count
into the resource manager, which allows the guest's own TGA parser to
populate the texture dimensions.

The route reaches the first maze and remains stable through the bounded probe:

1. Frame 775 enters the maze scene with 33 draws per frame.
2. Frame 800 shows the rendered maze in `READY` state with score `0`.
3. The scripted clickwheel quadrant inputs move Pac-Man and animate the
   ghosts; frame 880 shows score `30` and frame 890 shows score `40`.
4. The run continues through frame 1048 with changing maze frames, draw counts
   of 32-37, and no fatal signature.

This is the first PAC-MAN evidence of actual in-maze control in fliwheel. The
bounded capture run disabled audio output; the separate headed replay below
enabled the sink. Collision/life behavior, exit/save, physical speaker/mixer
behavior, persistence, full content, and long-run play remain open.

A matching headed replay with the desktop sink enabled emitted 12 mapped
`AudioEvent` records and 12 `played sound` receipts. The sink accepted
`start.wav`, `siren1.wav`, `eatopen.wav`, and `eatclose.wav` among the observed
events, with no decoder, sink, fault, or fatal records. This proves host
dispatch and decoding, not physical speaker output or mixer parity.

The normal pause route is now verified as well. A Menu edge at guest frame
1100 produces the `PAUSE` overlay with `RESUME GAME`, `VOLUME`, `OPTIONS`, and
`ABANDON GAME / SAVE & EXIT`. Select at frame 1180, after the overlay has
settled, resumes the maze: the capture is blank at frame 1202 and shows the
live maze again at frame 1203. The earlier immediate Select attempt at frame
1150 is retained as evidence that the menu needs time to settle before the
action edge is accepted.

## Reproduction

Build the release runner, then use the decrypted bundle:

```bash
cargo build --release --bin eapp

env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_AUDIO_TRACE=1 \
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

For the pause/resume follow-up, the same startup prefix was used with this
additional input script and stop frame:

```bash
FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:100-252,action:260-265,action:300-305,action:350-355,action:750-755,menu:1100-1105,action:1180-1185'
FLIWHEEL_EAPP_STOP_FRAME=1220
```

The pause overlay begins at frame 1102. The settled Select edge is accepted
at frame 1180; frame 1202 is the transition blank and frame 1203 is the live
maze again.

## Evidence

- Log: `/tmp/fliwheel_pacman_movement_20260828.log`
- Default-path promotion log: `/tmp/fliwheel_pacman_default_maze_20260828.log`
- Capture manifest: `/tmp/fliwheel_pacman_movement_20260828/manifest.tsv`
- Headed audio log: `/tmp/fliwheel_pacman_audio_headed_mapped_20260828.log`
- Pause/resume log: `/tmp/fliwheel_pacman_pause_resume_20260828.log`
- Pause/resume capture manifest: `/tmp/fliwheel_pacman_pause_resume_20260828/manifest.tsv`
- Visual captures include frames 800, 840, 880, and 890 in
  `/tmp/fliwheel_pacman_movement_20260828/`.
- The 880 and 890 captures visibly show the score changing from `30` to `40`
  while the maze and sprites remain coherent.
- The headless probe used `FLIWHEEL_EAPP_AUDIO_DISABLE=1`; the separate headed
  log proves mapped WAV dispatch through the desktop sink.

## Default-path boundary

The pre-promotion default regression is retained in
`/tmp/fliwheel_pacman_default_regression_20260828.log`; it reaches guest
initialization states 2-9 and faults at `PC 0x1801628c` before the maze. The
current normal path uses the measured title-specific completion contract, as
captured in `/tmp/fliwheel_pacman_default_maze_20260828.log`; pause/resume is
verified by `/tmp/fliwheel_pacman_pause_resume_20260828.log`. The remaining
gates are exit/save, collision/life behavior, physical audio/mixer behavior,
persistence, full content, and long-run play.
