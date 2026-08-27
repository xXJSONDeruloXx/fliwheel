# Zuma audio source/trigger ABI probe

Date: 2026-08-27 UTC  
Bundle: `44444` (`Zuma_1_1_2563298.bin`)  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`

## Purpose

The live Zuma board already produced projectile and collision activity, but the
generic audio observer did not identify the title's sounds. This probe follows
the title's own source-registration and trigger sequence and checks that the
HLE can preserve the guest source identity through to a host WAV path.

## Recovered contract

Static inspection and a startup import trace establish the following sequence:

1. Zuma calls `Audio:0` to allocate a source handle with a source type, slot,
   and table base.
2. It immediately starts an `AsyncFileIO:3` read for that source's WAV header.
   The 36 observed registrations resolve to the 36 embedded `audio/*.wav`
   paths in the bundle.
3. The live source helper configures the source with `Audio:13`, `Audio:14`,
   and `Audio:15`, then commits playback with `Audio:2` using the handle
   returned by `Audio:0`.

The HLE now binds each WAV path to the most recently allocated Zuma source,
and queues a host event only at the final `Audio:2` commit. The path is not
guessed from a generic resource ordinal, so the existing Tetris observer and
other title families are unaffected.

## Focused result

The deterministic board-entry/gameplay input from the [Zuma gameplay
probe](20260827_zuma_gameplay_probe.md) was replayed against the release
runner with the experimental live-GL path, `FLIWHEEL_AUDIO_TRACE=1`,
`EAPP_AUDIO_EVENT_TRACE=1`, and `EAPP_AUDIO_DISABLE=1`:

- 36 `AudioSourceMap` records were emitted, covering every registered source.
- 24 `AudioEvent` records were emitted with resolved host paths.
- The events include `zuma_button1.wav`, repeated `zuma_fireball1.wav`,
  `zuma_ballclick1.wav`, `zuma_chime1.wav`, and `zuma_chime2.wav`.
- The run completed without a fatal signature. The raw receipt is
  `/tmp/fliwheel_zuma_audio_events_20260827.log`.

This verifies title-specific source identity and guest-trigger timing in the
HLE event queue.

## Headed sink result

The same title-specific path was replayed with the desktop sink enabled. The
bounded run emitted 36 source maps, 23 resolved events, and 23 `played sound`
receipts before the external 75-second watchdog stopped it at the still-live
frame loop. The receipts include the fireball, ball-click, and both chime
assets, so rodio accepted and dispatched those decoded WAVs through the
headed sink. The raw receipt is
`/tmp/fliwheel_zuma_audio_headed_20260827.log`; the watchdog stop means this is
not a clean full-run timing result.

Neither run establishes physical output, mixer volume/pan, voice overlap, or
exact timing against an iPod recording. The deterministic ABI run disabled
the desktop sink, while the headed run proves host dispatch only.

## Next gates

- Replay the same sequence with the headed desktop sink and confirm rodio
- without the external watchdog once the UI/frame-stop path is made bounded.
- Compare timing, volume, pan, and overlap against reference captures.
- Continue aim-angle calibration and save/persistence investigation; those
  remain the larger blockers to calling Zuma playable.
