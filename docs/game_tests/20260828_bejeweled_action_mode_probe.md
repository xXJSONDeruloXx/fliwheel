# Bejeweled Action mode probe

Date: 2026-08-28 UTC  
Bundle: `55555`  
Executable: `Bejeweled_1_1_2563296.bin`

## Result

The normal Bejeweled menu path now has a reproducible Action-mode entry. A
positive wheel step moves the menu underline from `START CLASSIC GAME` to
`START ACTION GAME`; Select then enters the same built-in tutorial and a live
8×8 board. The first Action-board tap is accepted: the guest enters its swap
path at frame 1004, emits `swap.wav`, follows with `gotset.wav` and
`gemongem.wav`, shows a score of `50`, and reaches the guest's `Well done!`
overlay. Select dismisses that overlay back to a live Action board with the
vertical green timer bar. A separate follow-up still produces a rejected swap
and `bad.wav` without a fatal signature.

This is one accepted Action-mode match, not a claim that Action mode is
complete. A separate idle replay reached frame `4000` and showed the green
timer gauge decrementing: its top edge moved from `y=152` at frame `1160` to
`y=159` at frame `3986`. That run did not yet reach timeout/game-over.
Multiple moves, save persistence, and physical mixer parity remain open.

## Reproduction

The confirmed headed replay used the release runner and the decrypted bundle:

```text
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_EAPP_INPUT_SCRIPT='wheel=1:30-33,action:60-62,action:842-844,wheel=-1:882-982,bits=0x400000b0:1002-1003,action:1142-1144,wheel=1:1182-1207,bits=0x400000f0:1212-1213'
```

Receipt:

```text
/tmp/fliwheel_bej_action_mode_confirmed_late_20260828.log
/tmp/fliwheel_bej_action_mode_confirmed_late_20260828/
```

The late capture shows the Action timer bar and live board at frames 1160,
1200, 1214, 1230, 1265, and 1300. The run completed through frame 1349 with
78 draws per live frame and no fatal, panic, decoder, or sink error. Its first
Action match is recorded in
`/tmp/fliwheel_bej_action_mode_confirmed_late_20260828.log`: `swap.wav` at
frame 1004, `gotset.wav` at 1015, and `gemongem.wav` at 1022. The early-window
capture `/tmp/fliwheel_bej_action_early_capture_20260828/` shows the score-50
resolution effect; the later frame 1120 capture shows the `Well done!` prompt
before the settled Action board. The separate rejected follow-up emits
`swap.wav` at frame 1229 and `bad.wav` at frame 1251 in
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
