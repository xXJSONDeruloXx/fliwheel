# Zuma gameplay probe

Date: 2026-08-27 UTC  
Bundle: `44444`  
Purpose: verify that the corrected live board accepts clickwheel input far
enough to fire a projectile and produce an in-game collision/result effect.

## Result

The durable decrypted corpus completed a 400,000,000-cycle headless run with
exit `0` and no fatal signature. The replay reached the corrected
`LEVEL 1-1: SPIRAL OF DOOM` board, applied a wheel rotation at guest frames
`7300`-`7302`, and applied a Select edge at `7400`-`7402`. The captured frame
sequence shows:

| Guest frame | Observed state |
| ---: | --- |
| `7100` | Live board with coherent spiral path, frog, HUD, active chain, and score display |
| `7300` | Live board after the post-entry wheel packet; the board remains composed and interactive |
| `7400` | Green projectile visibly leaving the frog after the Select edge |
| `7410` | Projectile/chain animation continues with the board still coherent |
| `7420` | Red projectile/impact state and a bonus coin are visible; score/chain state has changed |

This is the first direct visual evidence of a Zuma gameplay action after the
renderer fix: input reaches the game, a projectile is launched, and the board
produces a collision/result animation. It does not yet establish controlled
aim-angle parity, repeatable multi-shot play, audio output, or persistence.
The initial score/effect visible at frame `7100` is close to the tutorial-close
edge, so the exact causal boundary for that first effect is intentionally not
used as the sole proof; the frame `7400`-`7420` sequence is the decisive probe.

## Reproduction

Corpus executable:

```text
/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/44444/Executables/Zuma_1_1_2563298.bin
```

The replay used the release binary from commit `52005b7` (source renderer fix
`b12cd60`) with sparse periodic capture:

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_STARTUP_CAPTURE_PERIOD=10
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=9500
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=2500
EAPP_AUDIO_DISABLE=1
EAPP_AUDIO_EVENT_TRACE=1
```

The complete input script was:

```text
action:700-702,action:1000-1002,action:1300-1302,action:1600-1602,action:1900-1902,action:2200-2202,action:2500-2502,action:2800-2802,action:3100-3102,action:3400-3402,action:3700-3702,action:4000-4002,action:4300-4302,action:4600-4602,wheel=3:4900-4902,action:5000-5002,wheel=-3:5300-5302,action:5400-5402,wheel=6:5700-5702,action:5800-5802,wheel=-6:6100-6102,action:6200-6202,action:6500-6502,action:6800-6802,action:7100-7102,wheel=3:7300-7302,action:7400-7402,wheel=-6:7600-7602,action:7700-7702,wheel=12:7900-7902,action:8000-8002,wheel=-12:8200-8202,action:8300-8302,wheel=24:8500-8502,action:8600-8602,wheel=-24:8800-8802,action:8900-8902,wheel=48:9100-9102,action:9200-9202
```

Run evidence is retained at:

```text
/tmp/fliwheel_zuma_gameplay_sparse.iDL3rl/run.log
/tmp/fliwheel_zuma_gameplay_sparse.iDL3rl/capture/manifest.tsv
/tmp/fliwheel_zuma_gameplay_sparse.iDL3rl/capture/startup_g007400_host000326806930_hashf8c8e6d241610714.ppm
/tmp/fliwheel_zuma_gameplay_sparse.iDL3rl/capture/startup_g007410_host000328600001_hashe6de342b46b70c8c.ppm
/tmp/fliwheel_zuma_gameplay_sparse.iDL3rl/capture/startup_g007420_host000330274155_hasha7f9ed423731b761.ppm
```

The manifest contains 8,688 guest-frame rows (`0`-`8687`), 2,922 presented
hashes, 3,125 hash changes, a 340-draw peak, and two zero-draw frames. The run
wrote 2,500 sparse/hash-change PPMs and logged no fatal signature. No
recognized `AudioEvent` was emitted; the host audio sink was disabled for this
headless probe, so audio remains open.

## Remaining gates

- repeatable wheel-to-aim angle mapping and shot direction;
- several shots with deterministic collision/chain behavior;
- headed audio output, timing, and mixer parity;
- save/load and persistence behavior.
