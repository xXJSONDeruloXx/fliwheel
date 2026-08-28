# Ms. PAC-MAN diagnostic gameplay probe

Date: 2026-08-27 UTC  
Bundle: `14004`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004`

## Result

The title-scoped completion path now gets Ms. PAC-MAN past the resource-loading
stall and through the guest's own menus. The run reached:

1. Name entry, including wheel-selected letters and the confirm checkmark.
2. The post-name help screen and main menu.
3. The Play Game menu and the in-game tutorial.
4. A complete Stage 1 maze with HUD, pellets, ghosts, lives, and score.

The gameplay probe then injected the clickwheel quadrant packet used by the
title. Pac-Man moved through the maze and the score advanced from `0` to `160`
in the captured run. This establishes a controllable gameplay path, but not
full completion: collision/life behavior, audio output, persistence, long-run
stability, and the normal non-probe resource contract still need work.

The behavior is diagnostic-only. The default run remains loading-only because
the four completion flags below are not enabled by the launcher or matrix.

## Reproduction

```bash
cargo build --release -p fliwheel-desktop --bin eapp

env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC0_COMPLETE=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC1_COMPLETE=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC2_COMPLETE=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC3_COMPLETE=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='action:100-105,wheel=1:125-130,action:145-150,wheel=1:165-170,action:185-190,wheel=-1:205-222,action:230-235,wheel=-1:245-262,action:280-285,action:340-345,action:380-385,action:430-435,action:600-605,bits=0x400000f0:650-680,bits=0x40000030:700-730,bits=0x40000070:750-780,bits=0x400000b0:800-830' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_mspacman_gameplay_input_20260827 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=900 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=900 \
  FLIWHEEL_EAPP_STOP_FRAME=900 \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004' \
  --headless --cycles 360000000
```

The raw quadrant values used by the run are `0x30` up, `0x70` left,
`0xb0` down, and `0xf0` right, with bit `0x40000000` marking a wheel touch.
They are injected into the compact `InputEvents:0` packet; ordinary button
events are still used for menu/action edges.

## Evidence

- Capture directory: `/tmp/fliwheel_mspacman_gameplay_input_20260827/`
- Log: `/tmp/fliwheel_mspacman_gameplay_input_20260827.log`
- Guest frame 620: full Stage 1 maze in `READY!` state, with HUD and lives.
- Guest frame 720: active maze with score `70`.
- Guest frame 840: active maze with score `160`.
- The run completed its bounded cycle budget without a fatal signature.

The earlier texture-only evidence remains in the
[texture-association probe](20260827_mspacman_texture_probe.md); this document
records the later post-resource and input result.
