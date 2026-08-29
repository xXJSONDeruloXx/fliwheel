# Cubis 2 audio/source mapping probe

Date: 2026-08-29  
Bundle: `99999`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/99999`

## Result

Cubis 2 uses the WAV-backed `Audio:0`/`Audio:13`/`Audio:14`/`Audio:15`/`Audio:2`
contract. The current HLE now binds all 21 registered source handles to the
title's effect files:

```text
bombexplosion.wav buttonclick.wav chaincubis.wav chainregular.wav
cubecollide.wav cubecrack.wav cubecrumble.wav cubedestroy.wav cubedrop.wav
cubeshoot.wav gameover.wav lasershoot.wav levelcomplete.wav levelstart.wav
osmosisgloop.wav timecountdown.wav warp-generate.wav warp-teleport.wav
perfect.wav levelstartanim-warp.wav levelstartanim-nowarp.wav
```

The source map is exercised by real guest trigger calls. The headless route
resolved, among others, `buttonclick.wav`, `cubeshoot.wav`,
`levelcomplete.wav`, and `levelstart.wav` into `AudioEvent` records. A separate
headed run reached the existing desktop `rodio` sink and logged `played sound`
for the same resolved paths. Both runs exited with code `0` and no fatal EAPP
signature.

## Reproduction

Headless source/event verification:

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:90-93,action:160-163,wheel=-1:200-214,action:220-223,action:300-303,action:350-353' \
FLIWHEEL_AUDIO_TRACE=1 \
EAPP_AUDIO_EVENT_TRACE=1 \
target/debug/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/99999' \
  --headless --cycles 12000000 \
  > '/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-audio-map-probe-20260829/run.log' 2>&1
```

The headed sink verification used the same startup/name-entry route without
`--headless` and ran for 8,000,000 cycles. Its receipt is:

```text
/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-audio-headed-probe-20260829/run.log
```

The board-rendering captures are retained separately at:

```text
/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-start-game-probe-20260829/
/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-board-interaction-probe-20260829/
```

## Boundary

This verifies source identity, effect-event routing, and headed sink dispatch.
It does not prove audible physical output timing, music (`g.m4a`) playback,
voice overlap/mixer parity, completed cube matches, game-over/level progression,
pause/return, or save/persistence behavior. Those remain the next Cubis 2
acceptance gates.

The control target is consistent with the contemporary
[Cubis 2 iPod description](https://www.mobygames.com/forum/3/thread/38900/igame-on-your-ipod/),
which describes wheel movement along the grid and center-button firing; the
[MobyGames screenshot set](https://www.mobygames.com/game/24363/cubis-2/screenshots/)
is the visual reference for the isometric field and HUD.
