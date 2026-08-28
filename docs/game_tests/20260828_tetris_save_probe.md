# Tetris save/persistence probe

Date: 2026-08-28  
Bundle: `66666` (`Tetris_1_1_2563292.bin`)

This probe follows a real gameplay path (multiple hard drops and a line-clear
setup) and then injects the Menu edge. It checks whether the guest performs a
save operation after gameplay rather than only loading the initial save slots.

## Reproduce

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_ASYNC3_COMPLETE=1 \
FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:15-16,wheel=37:30-31,action:45-46,action:78-79,action:100-101,action:150-151,action:210-211,action:230-231,action:260-261,wheel=-3:300-305,down:330-331,wheel=-4:360-370,down:390-391,wheel=1:420-425,down:450-451,wheel=-1:480-483,down:510-511,wheel=4:540-545,left:550-551,left:555-556,down:570-571,menu:700-705' \
FLIWHEEL_EAPP_STOP_FRAME=900 \
RUST_LOG='EAPP_IMPORT=info,EAPP=warn,EAPP_GL=warn,EAPP_AUDIO=warn,EAPP_INPUT=warn' \
target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/66666' \
  --headless --cycles 120000000 \
  > /tmp/fliwheel_tetris_menu_save_probe_20260828.log 2>&1
```

## Result

- The replay completed at the bounded stop with no fatal, panic, or skipped
  frame signature.
- Boot performed the expected request-object reads:
  `AsyncFileIO:3 prefs.sav` and `AsyncFileIO:3 game.sav`, each requesting
  4,096 bytes and finding the current zero-byte `.fliwheel-saves` fixture.
- The gameplay-plus-Menu portion issued no `AsyncFileIO:12`, `:14`, or `:16`
  direct save-handle calls, and no host-side save write was observed.
- The host files remained zero bytes after the run:
  `.fliwheel-saves/prefs.sav` and `.fliwheel-saves/game.sav`.

This keeps persistence below the current evidence ceiling. The runtime's
direct handle implementation now supports the observed read ABI, but a guest
save-write path has not yet been found or implemented. Historical MGCT/RPCT
files in `TETRIS_SAVE_FORMAT.md` remain useful format leads, not validated
fixtures for this run.

