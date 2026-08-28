# PAC-MAN name-entry probe

Date: 2026-08-27 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Name-entry result

PAC-MAN's ordinary resource path completes all 20 observed asynchronous
requests and reaches the guest's name-entry screen. The clickwheel path is
direction-sensitive: the positive scripted wheel direction advances the
character carousel and Select commits characters to the name field. A fresh
route visibly reaches `A`, `AB`, `ABG`, and `ABGL`, and a longer route reaches
the eight-character string `ABGLQVO5` with the next-character cursor still
active.

The name-entry screen is missing the check/arrow artwork seen in the sister
Ms. PAC-MAN title. The bundle contains `tex_menu1.tga` and `tex_ig.tga`, but
not the `tex_menu.tga` referenced by the executable. Supplying an isolated copy
of `tex_menu1.tga` under that missing name did not change the run, and the
guest did not issue an AsyncFileIO request for `tex_menu.tga` on this route.
That makes the absent asset a confirmed preservation gap, but not yet a proven
cause of the transition or startup fault.

## Follow-up gameplay gate

The guest's name-entry object stores delete at selector `0x2b` and confirm at
selector `0x2c`. A continuous positive wheel route reaches selector `0x2c`,
and a Select edge drives the actual confirm handler:

```text
wheel=1:100-252,action:260-265
```

The follow-up route reaches the informational prompt, then the guest main menu
with `PLAY GAME` selected, and then the `START GAME / MODE / STAGE / BACK`
screen:

```text
wheel=1:100-252,action:260-265,action:300-305,action:350-355
```

The start action was then tested with:

```text
wheel=1:100-252,action:260-265,action:300-305,action:350-355,action:750-755
```

The root object advances through guest initialization states `2`, `3`, `4`,
`5`, `6`, `7`, `8`, and `9`, but the first maze frame is not rendered. The
guest faults at `PC 0x1801628c` while reading offset `0x58` through a null
nested pointer. The experimental live-GL run faults at frame 785; the legacy
fill-color run reaches the same fault at frame 791 in the saved rerun. This
makes the current
boundary a missing/uninitialized guest object or service contract, rather than
a missed input edge or a live-GL-only failure.

Evidence:

- Confirm/menu route log: `/tmp/fliwheel_pacman_postname_route_20260827.log`
- Start-screen route log: `/tmp/fliwheel_pacman_play_route_20260827.log`
- Live-GL start-gate log: `/tmp/fliwheel_pacman_maze_pc_trace_20260827.log`
- Legacy-path start-gate log: `/tmp/fliwheel_pacman_maze_legacy_20260827.log`
- Menu capture: `/tmp/fliwheel-pacman-postname.Bg9iA2`
- Start-screen capture: `/tmp/fliwheel-pacman-play.30TuYa`

## Reproduction

The positive-wheel portion of the probe used:

```text
action:100-105,
wheel=1:125-130,
action:145-150,
wheel=1:165-170,
action:185-190,
wheel=1:205-222,
action:230-235,
wheel=1:245-262,
action:280-285,
wheel=1:295-312,
action:330-335,
wheel=1:345-362,
action:380-385,
wheel=1:395-412,
action:430-435,
wheel=1:445-462,
action:480-485,
wheel=1:495-512,
action:530-535
```

The corresponding bounded run used the normal live-GL environment, disabled
audio output, captured every guest frame, and stopped at frame 650:

```bash
env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  FLIWHEEL_EAPP_INPUT_SCRIPT='action:100-105,wheel=1:125-130,action:145-150,wheel=1:165-170,action:185-190,wheel=1:205-222,action:230-235,wheel=1:245-262,action:280-285,wheel=1:295-312,action:330-335,wheel=1:345-362,action:380-385,wheel=1:395-412,action:430-435,wheel=1:445-462,action:480-485,wheel=1:495-512,action:530-535' \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_pacman_name_variant_plus_long_20260827 \
  FLIWHEEL_STARTUP_CAPTURE_PERIOD=1 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=650 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=650 \
  FLIWHEEL_EAPP_STOP_FRAME=650 \
  target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA' \
  --headless --cycles 260000000
```

Evidence:

- Log: `/tmp/fliwheel_pacman_name_variant_plus_long_20260827.log`
- Captures: `/tmp/fliwheel_pacman_name_variant_plus_long_20260827/`
- The default resource run still ends with `req=20`, `callbacks=20`,
  `pending=0`, `staged=20`, and no fatal signature.
- The earlier default route can enter the guest's save/exit dialog, proving
  that ordinary button edges reach the title even though gameplay is not yet
  selected.

## Next gate

Identify which guest object should populate the null nested pointer during the
start-state initialization, then compare its resource/service setup with the
already-proven Ms. PAC-MAN diagnostic route. After that, verify maze rendering,
D-pad movement, collisions, audio, and persistence.
