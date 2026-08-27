# Sudoku RLB seek and setup milestone

Date: 2026-08-27 UTC  \
Bundle: `50513`  \
Repository: `fliwheel`  \
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`

This is a title-scoped investigation receipt. The completion gates below are
experimental and are not enabled by the default corpus-wide contract.

## Resource evidence

The staged file is:

```text
Games_RO/50513/Sudoku.rlb
size: 16,409,519 bytes
header: 00 00 00 00 00 00 00 00 05 00 00 00 ba 03 00 00 ...
```

The RLB directory has 236 entries and ends at `0x3ba`. Static tracing of the
Sudoku stream manager showed the following sequence after the header read:

```text
seek 0x8d381
read 153884 bytes
```

Before the seek fix, the HLE copied that read from the current post-header
offset. The title therefore received the wrong resource bytes. The
title-scoped AsyncFileIO:2 completion now updates the staged host-backed
offset for the guest's type-5 seek before servicing the type-3 read.

## Rendering evidence

The focused run used:

```text
EAPP_SUDOKU_ASYNC0_COMPLETE=1
EAPP_SUDOKU_ASYNC1_COMPLETE=1
EAPP_SUDOKU_ASYNC2_COMPLETE=1
FLIWHEEL_EAPP_ASYNC0_RESULT=length
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
```

The late setup capture initially reported two skipped draws at centered
half-texel edges. The renderer was requiring `ceil(640.5) == 641` pixels for a
valid 640-pixel upload, and likewise rejected the 425-pixel upload ending at
`425.5`. The shared containment helper now subtracts the half-texel before
rounding the required extent.

After the fix:

- the former `handle=0x19` draw rasterized with coverage `40000`;
- the former `handle=0x0d` draw rasterized with coverage `47524`;
- the focused GL log contained no `draw... skipped` lines for that scene;
- a late capture showed a coherent wooden background, ornate panel, `PLAYER
  NAME` heading, current letter, alphabet row, controls, and validation text.

## Input milestone

With `action:8200-8230`, the guest advances from `PLAYER NAME` to `GAME
SETUP`, showing `Play!`, `Difficulty: Easy`, and `Error Checking: On`. A second
action at `8500-8505` is consumed and causes a real animated transition, but
the guest later returns to the name/setup flow. No puzzle grid has been
verified, so Sudoku is still not playable or near-perfect.

The invalid-surface loop after the setup transition also remains open. The
next target is the title's Play/start-state contract, followed by puzzle-grid
controls, sound, and save behavior.
