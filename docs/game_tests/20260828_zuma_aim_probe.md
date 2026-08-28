# Zuma directional aim probe

Date: 2026-08-28 UTC  
Bundle: `44444` (`Zuma_1_1_2563298.bin`)

## Result

The existing generic clickwheel angle path produces directional gameplay
changes in Zuma. Two fresh runs used the same tutorial/board-entry schedule
and the same single shot edge, changing only the signed wheel packet before
the shot:

| Wheel packet | Shot/result receipt | Visual result |
| ---: | --- | --- |
| `wheel=+3` at `7000..7002`, Select at `7100..7102` | Frames `7103..7106`; `+40` result | Impact/effect travels into the upper-right chain region |
| `wheel=-3` at `7000..7002`, Select at `7100..7102` | Frames `7104..7110`; `+30` result | Impact/effect travels into the upper-left chain region |

The separate processes generate different marble layouts, so this is not a
claim of pixel-identical board determinism. It is, however, a reproducible
sign-sensitive response: positive and negative wheel packets reach the live
frog and produce mirrored left/right shot outcomes. Both runs reached their
bounded stop cleanly with no fatal, panic, decoder, or sink error.

This matches the documented iPod control model: the click wheel rotates the
frog and Select fires the marble. The contemporary [Macworld Zuma review]
(https://www.macworld.com/article/181887/zuma.html) also calls out the
conversion's imprecise aiming, so directional response is a useful parity
milestone but not an accuracy/completion claim.

## Reproduction

Both runs used the release runner, the durable decrypted corpus, live GL HLE,
and a late capture window. The common entry schedule was:

```text
action:700-702,action:1000-1002,action:1300-1302,action:1600-1602,
action:1900-1902,action:2200-2202,action:2500-2502,action:2800-2802,
action:3100-3102,action:3400-3402,action:3700-3702,action:4000-4002,
action:4300-4302,action:4600-4602,action:4900-4902,action:5200-5202,
action:5500-5502,action:5800-5802,action:6100-6102,action:6400-6402,
action:6700-6702
```

The positive run appended:

```text
wheel=3:7000-7002,action:7100-7102
```

Receipt:

```text
/tmp/fliwheel_zuma_aim_pos_20260828.log
/tmp/fliwheel_zuma_aim_pos.FYhFZ9/manifest.tsv
/tmp/fliwheel_zuma_aim_pos.FYhFZ9/startup_g007103_host000258857295_hash045b280c2d6e3a83.ppm
```

The negative run appended:

```text
wheel=-3:7000-7002,action:7100-7102
```

Receipt:

```text
/tmp/fliwheel_zuma_aim_neg_20260828.log
/tmp/fliwheel_zuma_aim_neg.ftFFCA/manifest.tsv
/tmp/fliwheel_zuma_aim_neg.ftFFCA/startup_g007104_host000289032924_hash297fb96bb76ea45b.ppm
```

The positive capture contains 7,251 manifest rows (`0..7250`) and 250 PPM
frames in the late window. The negative capture contains 7,121 rows
(`0..7120`) and 30 late-window PPM frames. The source receipts retain the
full per-frame images used to identify the directional impact regions.

## Interpretation and remaining gates

- The previously recorded `0x180bf898`/`0x180bfaa0` angle stores are not
  dead writes: changing their signed input changes the live shot result.
- No Zuma-specific packet remap is justified; the generic 8-bit signed
  packet path is already functionally connected to aiming.
- Exact wheel-to-angle scale, center/origin calibration, aim precision,
  multiple-shot progression, headed audio timing/mixer behavior, and save/load
  remain open.
