# Sims rectangle-target paletted texture fix

Date: 2026-08-27 UTC
Bundles: `1500C` (Sims Bowling), `1500E` (Sims Pool)
Runner: `target/debug/eapp` with experimental live GL HLE

## Finding

The Sims engine uses `OpenGLES:19` for its indexed artwork. Bowling's first
post-title upload is:

```text
r0=0x84f5  r2=0x8b96  r3=0x162
height=25  image_size=9874  source=0x10015b90
```

`0x84f5` is the rectangle-texture target and `0x8b96` is
`GL_PALETTE8_RGBA8_OES`. The payload length is exactly `1024 + 354 * 25`:
1024 bytes of RGBA palette followed by one index per texel. The old HLE only
recognized `GL_TEXTURE_2D` (`0x0de1`), so this call fell through to the
render-server placeholder and the `0x27` material had no matching upload.

The decoder now accepts either texture target when the format is
`GL_PALETTE8_RGBA8_OES`. The change is shared and remains inside the
experimental live-GL path.

## Default-path evidence

These runs deliberately omitted `FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE`; the
whole-file Sims RLB completion remains diagnostic-only.

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 FLIWHEEL_EAPP_STOP_FRAME=8 \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_sims_default_20260827_04 \
target/debug/eapp /path/to/Games_RO/1500C --headless --cycles 30000000

FLIWHEEL_EXPERIMENTAL_GL_HLE=1 FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 FLIWHEEL_EAPP_STOP_FRAME=8 \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_sims_default_20260827_05 \
target/debug/eapp /path/to/Games_RO/1500E --headless --cycles 30000000
```

| Bundle | Exit | Capture rows | Unique hashes | Hash changes | Max draws | Zero-draw rows | Upload result |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `1500C` Bowling | 0 | 7 | 5 | 5 | 2 | 2 | `compressed_upload` `354x25`, texture name `2` |
| `1500E` Pool | 0 | 6 | 4 | 4 | 2 | 1 | ordinary `297x75` texture upload, unchanged |

Bowling's first post-title capture now contains a legible `The` text element;
Pool shows the corresponding colored `The` element through its ordinary atlas
path. Neither title reaches its menu or gameplay scene yet, and neither run
reported a fatal signature.

## Diagnostic RLB probe

The optional Sims whole-file completion flag stages the 19,997,809-byte
Bowling RLB or 19,440,133-byte Pool RLB and runs the guest completion callback
chain. Both callbacks return cleanly, but the titles remain in `frame_state=1`
and do not issue a downstream scene transition. This is retained as
reverse-engineering evidence, not as a default fix:

```text
FLIWHEEL_EAPP_SIMS_ASYNC0_COMPLETE=1
```

The next Sims target is therefore the resource/scene handoff after the
post-title renderer calls, rather than enabling this completion globally.

## Remaining state

The shared target fix improves asset coverage but does not establish
playability. Both Sims titles remain partial: coherent launch screen, a small
post-title text draw, and no verified menu, input, audio, or persistence parity.
