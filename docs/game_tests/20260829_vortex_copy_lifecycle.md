# Vortex PR #3 copy and input lifecycle

Date: 2026-08-29 UTC  
fliwheel commit: `6b6a901`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/12345`  
Oracle: `/tmp/ipod-emulator-pr3/target/release/play`

## Reproducible state

The direct runner writes Vortex's `options`, `stats`, `quicka`, and `quickb`
files into the supplied bundle directory. A comparison must therefore use a
fresh temporary copy of the bundle with those generated files excluded; the
downloaded corpus is left untouched. The fliwheel copy is named `12345` so
the title-gated PR3 path is enabled.

The permanent oracle harness is
[`pr3_vortex_name_copy_checkpoints.script`](../../scripts/oracle/pr3_vortex_name_copy_checkpoints.script).
It captures the initial copy boundary, the title-to-name transition, wheel
selection, and the settled name-entry frame.

## Copy contract

The direct oracle's `OpenGLES:21` calls all come from `0x18014e00` and occur at
its internal frame labels 69, 501, and 504. A same-label fliwheel run reaches
the same callsite at internal labels 73, 500, and 503. The one-frame label
offset is a runner bookkeeping difference: the oracle labels its presented
frame during the import path, while fliwheel increments its guest-frame
counter at the frame-vector return.

Both implementations copy a 320x240 rectangle from `(0,0)` with the GL
bottom-left row convention. The remaining name-entry background delta is the
rotating Vortex title content present in the source framebuffer at the copy
boundary, not an unresolved texture name or a missing `glCopyTexImage2D`
handler. A fixed clock seed was tested and rejected because it worsened the
background phase and changed the composition.

## Input contract

The direct oracle reports the Vortex flags word at `0x18063e5c`, with Select
as bit `0x01` and the tested Menu bit as `0x10`. fliwheel mirrors those
title-local states under `FLIWHEEL_VORTEX_PR3_GL165=1` and keeps the generic
event-list path disabled for this title. The reproducible probes are:

- [`pr3_vortex_start_game_checkpoints.script`](../../scripts/oracle/pr3_vortex_start_game_checkpoints.script)
  for name entry and `DONE`.
- [`pr3_vortex_start_dense.script`](../../scripts/oracle/pr3_vortex_start_dense.script)
  for the Level 1 start transition.
- [`pr3_vortex_button_early.script`](../../scripts/oracle/pr3_vortex_button_early.script)
  and [`pr3_vortex_menu_return.script`](../../scripts/oracle/pr3_vortex_menu_return.script)
  for non-Select buttons and return behavior.

The name-entry UI reaches 46 draws per frame in fliwheel, including the
selected-letter state after wheel/select input. Gameplay checkpoints already
show near-identical rendered brick-field frames after the Level 1 transition;
the remaining acceptance work is pause/return, full button semantics,
audio-output mapping/timing, and traversal beyond the currently scripted
content.

## Boundary

This is still a direct decrypted-eApp/HLE comparison. Neither runner boots an
IPSW firmware image, and this evidence does not establish perfect or complete
playability.
