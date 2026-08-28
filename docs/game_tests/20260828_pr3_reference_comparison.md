# PR #3 direct-HLE reference comparison

Date: 2026-08-28  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/`  
Reference checkout: `/tmp/ipod-emulator-pr3` at `96bfe90`  
fliwheel baseline before this probe: `593ed09`

## What is being compared

PR #3 has two different execution layers:

1. `play <game.bin>` runs a decrypted game eApp inside a synthetic
   RetailOS-like machine. It supplies HLE implementations for input, files,
   audio, OpenGL, timing, allocation, and the framebuffer.
2. The repository also has a separate full-firmware path. That path needs a
   matching NOR dump and the IPSW payload, and it was not used here because no
   matching 1 MiB NOR dump is available.

The fliwheel comparison in this report is against layer 1. Neither side is
booting the downloaded firmware image for these game results.

## LOST direct-runner comparison

The useful oracle trace was captured with the PR runner's 14-frame script. Its
file sequence was:

```text
rserver.bin       AsyncFileIO:3 whole-file load
options.sav       AsyncFileIO:0 -> completion -> AsyncFileIO:1
/l                AsyncFileIO:0 -> completion
/l                AsyncFileIO:2 read -> completion callback 0x1803b1d8
/d5               AsyncFileIO:0 -> completion
OpenGLES:19,149,137,40,37
```

fliwheel now follows that same sequence. The first fliwheel draw frame is
frame 14, with six rasterized quads and presented hash
`0x485b1fd3c5a8d965`. The corresponding PR capture is the GameLoft splash;
the images are visually equivalent, although the two software rasterizers
are not byte-identical at the antialiased logo edges.

The fliwheel 120-frame extension remains stable and advances through
resource-driven screens. A 362-frame no-input run completed without a fatal
signature. Its later draw stream is not yet a parity result: it contains
unsupported mode-7 primitive records and has not been checked against a
matching PR no-input capture, so LOST remains partial rather than playable.

## Changes that enabled the comparison

- Added a separate zeroed `0x19000000` game heap and matched the PR's initial
  playlist/context layout for LOST.
- Matched LOST's frame-context placement (`manager` and `manager + 0x100`)
  and its reason-byte lifecycle: initialization, one zero-reason transition,
  then steady reason 1.
- Matched the direct runner's generic async open/op/read completion fields,
  including stream handles, staged file offsets, byte counts, and callback
  arguments.
- Added an opt-in fixed clock (`FLIWHEEL_EAPP_FIXED_CLOCK=1`) for deterministic
  comparisons. Normal wall-clock execution remains available.
- Kept the older vendored ARMv4T core after an A/B: it reaches the same LOST
  callbacks, OpenGLES calls, six-draw frame, and presented hash. No external
  `/tmp` CPU dependency is part of the fliwheel change.

## Current per-title reference note

The PR runner's strongest long-run demonstrations remain Bowling and Pool
title menus, SAT name entry, Royal Solitaire's menu, and LOST's scripted
resource/input session. They are useful behavioral oracles, not proof that
fliwheel has parity. fliwheel's current matrix remains the authority for its
own status; none of the 20 decrypted bundles is certified fully playable.

## Next LOST gates

1. Decode and rasterize the remaining mode-7 submissions without silently
   skipping them.
2. Re-run matched scripted wheel/button sequences against PR #3 and compare
   scene transitions, not only first-frame images.
3. Trace the episode/resource state machine through the remaining `d*`, `l*`,
   localized resource, and music files.
4. Verify input, save/reload, sound effects, and long-run stability before
   changing the status to playable.
