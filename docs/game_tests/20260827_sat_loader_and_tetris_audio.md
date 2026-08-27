# SAT loader and Tetris audio checkpoint

Date: 2026-08-27 UTC  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`

This checkpoint records the follow-up probes behind the current 20-title
matrix. It deliberately separates a coherent rendered screen from a proven
content transition, and a host sink acceptance from physical mixer parity.

## SAT Prep family

Reading (`11050`), Writing (`11051`), and Mathematics (`11052`) all follow the
same measured loader path. The first two resource requests load
`rserver.bin` and `Fonts/Roman/fontinfo.txt`; their callbacks complete, and the
guest then settles into its steady state-6 per-frame path. No further
`Data/*`, `Graphics/*`, or question/content request appears in the bounded
trace.

The live renderer is coherent at this boundary:

- draw 1 is the full 320x240 SAT Prep screen;
- draw 2 is the 34x34 A8 spinner/font tile at approximately `(143,8)`;
- the screen shows the Kapland logo, `SAT PREP 2008`, and the title-specific
  `READING`, `WRITING`, or `MATHEMATICS` label.

The Reading trace also tested `Audio:47` return values `1` and `2`, and a
scripted input edge. Neither changed the state-6 loader or framebuffer. Those
return overrides are therefore not part of the runtime. The current boundary
is a missing or unobserved SAT content/runtime handoff, not a generic
renderer failure.

Evidence:

```text
/tmp/fliwheel_sat_progress2_20260827.log
/tmp/fliwheel_sat_capture_20260827.iKWTe9/
/tmp/fliwheel_sat_audio47_one_20260827.log
/tmp/fliwheel_sat_audio47_two_20260827.log
/tmp/fliwheel_11051_probe_20260827.P45lgZ/
/tmp/fliwheel_11052_probe_20260827.hX7p5u/
```

## Tetris sound and line-clear probe

The corrected 80M-cycle replay used the durable `66666` bundle and a sequence
of controlled wheel placements plus hard drops. It exited with code `0`,
produced 75 recognized resource events, and had no fatal signature:

| Resource event | Count | Result |
| --- | ---: | --- |
| `Drop.wav` | 12 | verified after scripted hard drops |
| `Lock.wav` | 5 | verified after later natural locks |
| `Clear.wav` | 0 | not reached by this placement schedule |

The absence of `Clear.wav` is a test boundary, not proof that the guest's line
clear logic or the HLE is wrong: the captured placement schedule did not yet
form a confirmed full row through the current visible board state.

A separate headed run exited with code `0` and logged all three initial
`Menu.wav` events as `played sound` after the persistent `rodio` sink accepted
them. This verifies the current host decode/sink path for the mapped WAV
resource. It does not yet establish acoustic timing, overlap, volume, or
mixer parity with an iPod.

Evidence:

```text
/tmp/fliwheel_tetris_audio_probe_20260827.bf9FtH/run.log
/tmp/fliwheel_tetris_headed_audio_20260827.UzIsc5/run.log
```

### Follow-up line-clear receipt

A calibrated five-piece replay subsequently placed Z/O/S/L/T through the
guest's normal input path. The headless trace emitted `Drop.wav` and
`Clear.wav` at frame 570 and entered the guest row-clear path at frame 576.
The corresponding headed replay logged `played sound` for `Clear.wav` through
the persistent sink. See the dedicated [line-clear probe](20260827_tetris_line_clear_probe.md)
for the exact script and board-state evidence; this supersedes the earlier
`Clear.wav = 0` result for the current Tetris status.

Tetris remains the strongest title, but its full parity gates still include
additional line-clear sequences, wall/kick behavior, piece sequencing,
save/load, and long-run visual/audio comparison.
