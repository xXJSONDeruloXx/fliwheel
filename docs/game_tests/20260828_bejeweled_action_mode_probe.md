# Bejeweled Action mode probe

Date: 2026-08-28 UTC  
Bundle: `55555`  
Executable: `Bejeweled_1_1_2563296.bin`

## Result

The normal Bejeweled menu path now has a reproducible Action-mode entry. A
positive wheel step moves the menu underline from `START CLASSIC GAME` to
`START ACTION GAME`; Select then enters the same built-in tutorial and a live
8×8 board. The live Action frame visibly contains the vertical green timer
bar, which distinguishes it from the Classic route. The run also produced a
rejected swap and `bad.wav` without a fatal signature.

This is mode-entry and live-rendering evidence, not a claim that Action mode
is complete. Timer expiry, an accepted Action-mode match, multiple moves,
game-over behavior, save persistence, and physical mixer parity remain open.

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
78 draws per live frame and no fatal, panic, decoder, or sink error. The audio
receipt includes `swap.wav` at frame 1215 and `bad.wav` at frame 1239.

The menu-direction A/B receipts are:

```text
/tmp/fliwheel_bej_menu_step_neg_20260828/
/tmp/fliwheel_bej_menu_step_pos_20260828/
```

The negative pulse leaves `START CLASSIC GAME` selected; the positive pulse
underlines `START ACTION GAME`.
