# Hold'em sound-bank source and playback boundary

Date: 2026-08-29 UTC  
Repository: `fliwheel`  
Bundle: `33333` (`HoldEm_1_1_2563291.bin`)

## Result

The HLE now understands Hold'em's `Sounds/sounds.blob` container. The file is
1,834,556 bytes and begins with a 27-entry little-endian length table. Each
entry is a valid PCM RIFF/WAVE record; records whose RIFF size is two bytes
short of the table length have the expected zero fill for four-byte alignment.

A fresh 70,000,000-cycle default-table route:

- validated and extracted all 27 bank records;
- emitted `AudioSourceMap` records for bank indices `0..26`;
- completed with exit code `0`;
- produced no fatal EAPP signature.

The extracted WAVs are kept with the external test corpus under:

```text
/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/33333/.fliwheel-saves/holdem-sfx/
```

The raw trace is retained externally at:

```text
/Volumes/NO NAME/fliwheel-runs-20260829/holdem-audio-map-fixed3/run.log
```

## Reproduction

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:100-105,action:120-125,action:140-145,action:160-165,action:180-185,action:200-205,action:220-225,wheel=104:240-240,wheel=4:360-360,wheel=4:400-400,wheel=4:440-440,action:480-485,action:800-805,action:820-825,action:840-845,action:900-905' \
EAPP_AUDIO_EVENT_TRACE=1 \
target/debug/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/33333' \
  --headless --cycles 70000000 \
  > '/Volumes/NO NAME/fliwheel-runs-20260829/holdem-audio-map-fixed3/run.log' 2>&1
```

## Boundary

This proves the container parser, aligned chunk boundaries, extraction, and
startup source identity. The route did not call guest `Audio:2`, so the
desktop `rodio` sink had no Hold'em playback event to dispatch in this run.
This is not yet evidence of complete audible hand play, correct overlap, or
physical output timing. The next audio probe must drive a route that reaches a
Hold'em playback commit and compare it with the iPod reference behavior.
