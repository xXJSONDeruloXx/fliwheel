# Vortex PR #3 direct-HLE color-array probe

Date: 2026-08-28 UTC  
fliwheel commit: `c3e6c84`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/12345`  
Oracle: `/tmp/ipod-emulator-pr3` direct `play` runner at PR #3

## Scope

This probe compares the decrypted Vortex eApp path only. It does not boot an
iPod firmware image. The downloaded IPSW files remain useful for the separate
firmware-boot path, but they are not involved in this result.

## Evidence

The context/allocator A/B matched the PR runner's early heap addresses. With
the title-scoped PR-compatible `OpenGLES:165` and unmapped-memory behavior,
both runners reached the same resource sequence:

```text
Backgrounds/circuits.ipd
Backgrounds/bgAlpha.ipd
Backgrounds/lava.ipd
Backgrounds/circuits_Door1.ipd
```

The late frame shape was 165 draws: a 320x240 background, the 128x128 center
element, the 204x108 ring elements, and the text glyphs. Texture selection and
asset loading were therefore already aligned.

The visual mismatch came from the guest's third enabled array. Vortex uses:

```text
array 0: position, 4 x GL_FIXED
array 1: UV,       2 x GL_FIXED
array 2: color,    4 x GL_FIXED
stride: 40 bytes
```

PR #3 interpolates array 2's RGB values across each triangle. fliwheel now
decodes the same array for DrawArrays, DrawElements, and triangle strips and
passes it to the generic software rasterizer. The change is not Vortex-only.

## Result

A 70M-cycle-style late capture on the new path reached guest frame 623 before
the bounded CPU budget ended. A focused capture at guest frames 610-612 shows
the full-color Vortex title composition: green circuit background, colored
ring segments, Vortex logo, and `PRESS SELECT`. The run produced 165 draws per
frame and no fatal signature.

The core library unit tests passed. Two older replay fixture assertions remain
red in the existing `eapp_gl_decode` integration test
(`orientation_helpers_respect_corner_markers_and_global_vertical_origin` and
`replay_frame4_produces_complete_artifact_and_hash`) on both `c3e6c84` and its
parent `8b05a7f`; they predate this color-array change and are tracked
separately.

## Open gates

- Send the Select/input transition and capture the first content scene.
- Compare Vortex controls and sound events against the direct PR runner.
- Extend the run through content and pause/return paths before considering the
  title playable.
