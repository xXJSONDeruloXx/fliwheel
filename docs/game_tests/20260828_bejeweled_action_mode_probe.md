# Bejeweled Action mode probe

Date: 2026-08-28 UTC  
Bundle: `55555`  
Executable: `Bejeweled_1_1_2563296.bin`

## Result

The normal Bejeweled menu path now has a reproducible Action-mode entry. A
positive wheel step moves the menu underline from `START CLASSIC GAME` to
`START ACTION GAME`; Select then enters the same built-in tutorial and a live
8×8 board. Two successive Action-board taps are accepted: the first enters
the swap path at frame 1004, emits `swap.wav`, follows with `gotset.wav` and
`gemongem.wav`, shows a score of `50`, and reaches the guest's `Well done!`
overlay. Select dismisses that overlay back to a live Action board with the
vertical green timer bar. A second valid tap at frame 1215 repeats the
swap/refill path, emits `gotset.wav`, `gemongem.wav`, and `combo2.wav`, and
reaches another score-50 `Well done!` overlay. A separate follow-up still
produces a rejected swap and `bad.wav` without a fatal signature.

This is two accepted Action-mode matches, not a claim that Action mode is
complete. A separate idle replay reached frame `4000` and showed the green
timer gauge decrementing: its top edge moved from `y=152` at frame `1160` to
`y=159` at frame `3986`. That run did not yet reach timeout/game-over.
Further moves, timeout/game-over, save persistence, and physical mixer parity
remain open.

## Reproduction

The confirmed headed replay used the release runner and the decrypted bundle:

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:30-33,action:60-62,action:842-844,wheel=-1:882-982,bits=0x400000b0:1002-1003,action:1142-1144,wheel=1:1182-1207,bits=0x40000070:1212-1213'
```

Receipt:

```text
/tmp/fliwheel_bej_action_match2_accepted_20260828.log
/tmp/fliwheel_bej_action_match2_accepted_20260828/
```

The dense late capture in
`/tmp/fliwheel_bej_action_match2_accepted_20260828/` covers frames 1180–1399.
The first Action match is recorded at `swap.wav` frame 1004,
`gotset.wav` frame 1015, and `gemongem.wav` frame 1021. The second accepted
match is recorded at `swap.wav` frame 1215, `gotset.wav` frame 1237,
`gemongem.wav` frames 1258/1262, and `combo2.wav` frame 1263. Its captures
show the score-50 resolution state at frame 1237, the refill in progress at
frame 1258, and the second `Well done!` prompt at frame 1300. The run exited
at frame 1400 with no fatal, panic, decoder, or sink error. The early-window
capture `/tmp/fliwheel_bej_action_early_capture_20260828/` retains the first
score-50 resolution. The separate rejected follow-up emits `swap.wav` at
frame 1229 and `bad.wav` at frame 1251 in
`/tmp/fliwheel_bej_action_match2_20260828.log`.

The menu-direction A/B receipts are:

```text
/tmp/fliwheel_bej_menu_step_neg_20260828/
/tmp/fliwheel_bej_menu_step_pos_20260828/
```

The negative pulse leaves `START CLASSIC GAME` selected; the positive pulse
underlines `START ACTION GAME`.

## Timer progression replay

The idle Action-mode timer check used the same menu/tutorial route, stopped at
guest frame `4000`, and captured only the late window:

```text
FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:30-33,action:60-62,action:842-844,wheel=-1:882-982,bits=0x400000b0:1002-1003,action:1142-1144'
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_bej_action_timer_late.amJm8t
FLIWHEEL_STARTUP_CAPTURE_PERIOD=30
FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=3400
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=4000
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=300
FLIWHEEL_EAPP_STOP_FRAME=4000
```

Receipt:

```text
/tmp/fliwheel_bej_action_timer_late_20260828.log
/tmp/fliwheel_bej_action_timer_late.amJm8t/manifest.tsv
/tmp/fliwheel_bej_action_timer_late.amJm8t/startup_g003400_host000113103748_hashcab646e416b5be26.ppm
/tmp/fliwheel_bej_action_timer_late.amJm8t/startup_g003986_host000125300495_hash295886683d90deb1.ppm
```

The run produced 4,001 manifest rows, 300 late PPMs, and no fatal, panic,
decoder, or sink error signature. The bar remains visible and shrinks slowly,
but the time-up sound and game-over state are not yet observed.
