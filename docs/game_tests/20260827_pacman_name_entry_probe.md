# PAC-MAN name-entry probe

Date: 2026-08-27 UTC  
Bundle: `AAAAA`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/AAAAA`

## Result

PAC-MAN's ordinary resource path completes all 20 observed asynchronous
requests and reaches the guest's name-entry screen. The clickwheel path is
direction-sensitive: the positive scripted wheel direction advances the
character carousel and Select commits characters to the name field. A fresh
route visibly reaches `A`, `AB`, `ABG`, and `ABGL`, and a longer route reaches
the eight-character string `ABGLQVO5` with the next-character cursor still
active.

The title does not reach its main menu or maze in this probe. The name-entry
screen is also missing the check/arrow artwork seen in the sister Ms. PAC-MAN
title. The bundle contains `tex_menu1.tga` and `tex_ig.tga`, but not the
`tex_menu.tga` referenced by the executable. Supplying an isolated copy of
`tex_menu1.tga` under that missing name did not change the run, and the guest
did not issue an AsyncFileIO request for `tex_menu.tga` on this route. That
makes the absent asset a confirmed preservation gap, but not yet a proven
cause of the state-transition failure.

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

Determine how PAC-MAN selects its name-confirm control and whether the missing
menu atlas is loaded only after that transition. Then reach `START GAME` and
compare the shared maze/input path with the already-proven Ms. PAC-MAN
diagnostic route.
