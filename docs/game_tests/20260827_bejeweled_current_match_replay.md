# Bejeweled current match replay

Date: 2026-08-27 UTC  
Bundle: `55555`  
Executable: `Bejeweled_1_1_2563296.bin`

## Result

The known deterministic input still reaches the live 8x8 board on the current
tree and performs an accepted swap. The board changes and refills, and the
guest emits the match-resolution sound `audio/combo2.wav`. This is a fresh
current-tree replay, not a reuse of the earlier 2026-08-26 capture.

The bounded run completed 1,350 guest frames with 372 unique presented hashes,
755 hash changes, a 185-draw peak, two zero-draw frames, and no fatal
signature. The relevant audio sequence includes the initial board setup,
selector movement, an accepted swap, and the later match path:

```text
frame 1172  audio/swap.wav
frame 1196  audio/gotset.wav
frame 1217  audio/gemongem.wav
frame 1224  audio/combo2.wav
```

## Reproduction

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_EAPP_INPUT_SCRIPT='action:18-20,action:800-802,wheel=-1:840-940,bits=0x400000b0:960-961,action:1100-1102,wheel=1:1140-1165,bits=0x400000f0:1170-1171'
```

Receipt:

```text
/tmp/fliwheel_bejeweled_match_current4_20260827.log
/tmp/fliwheel_bejeweled_match_current4_20260827/
```

The sparse current-tree captures around the resolution window are:

```text
/tmp/fliwheel_bejeweled_match_current4_20260827/frame_1200.png
/tmp/fliwheel_bejeweled_match_current4_20260827/frame_1224.png
/tmp/fliwheel_bejeweled_match_current4_20260827/frame_1250.png
```

These show the board before the refill, the partially resolved board, and the
post-resolution board/score overlay. `combo3.wav`, excellent-mode audio,
save persistence, and longer multi-move play remain unverified.
