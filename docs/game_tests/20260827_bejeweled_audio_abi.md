# Bejeweled WAV source and trigger ABI

Date: 2026-08-27 UTC
Bundle: `55555`
Executable: `Bejeweled_1_1_2563296.bin`
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`

## Recovered contract

Bejeweled registers 27 ordinary WAV sources with `Audio:0`. Each registration
is immediately followed by an `AsyncFileIO:3` 44-byte header read, so the
source handle can be bound to the resolved host WAV without guessing from the
slot number. The observed handle-to-asset order is:

| Handle | Slot | Asset |
| ---: | ---: | --- |
| `0x01` | 3 | `audio/bad.wav` |
| `0x02` | 1 | `audio/click.wav` |
| `0x03` | 2 | `audio/gotset.wav` |
| `0x04` | 7 | `audio/gotsetbig.wav` |
| `0x05` | 0 | `audio/gemongem.wav` |
| `0x06` | 5 | `audio/explode.wav` |
| `0x07` | 6 | `audio/warning.wav` |
| `0x08` | 9 | `audio/go_en.wav` |
| `0x09` | 28 | `audio/whirlpool_lt_en.wav` |
| `0x0a` | 29 | `audio/whirlpool_rt_en.wav` |
| `0x0b` | 11 | `audio/gameover_en.wav` |
| `0x0c` | 12 | `audio/gamestart.wav` |
| `0x0d`-`0x11` | 13-17 | `audio/combo2.wav` through `audio/combo6.wav` |
| `0x12`-`0x16` | 18-22 | `audio/combo2big.wav` through `audio/combo6big.wav` |
| `0x17` | 23 | `audio/selector.wav` |
| `0x18` | 24 | `audio/swap.wav` |
| `0x19` | 25 | `audio/excellent_en.wav` |
| `0x1a` | 26 | `audio/timeup_en.wav` |
| `0x1b` | 27 | `audio/nomoremoves_en.wav` |

Gameplay configures a source with `Audio:13`, `Audio:14`, and `Audio:15`, then
commits it with `Audio:2`. Fliwheel now retains the resolved WAV path on the
synthetic handle and emits one host audio event at that final trigger. This
behavior is title-scoped to the two PopCap bundles that have this measured
contract: Zuma (`44444`) and Bejeweled (`55555`).

## Focused replay

The exact deterministic input used for the historical Bejeweled swap receipt
was replayed with the release runner and `EAPP_AUDIO_EVENT_TRACE=1`:

```text
action:18-20,action:800-802,wheel=-1:840-940,
bits=0x400000b0:960-961,action:1100-1102,
wheel=1:1140-1165,bits=0x400000f0:1170-1171
```

Receipt: `/tmp/fliwheel_bejeweled_audio_events_20260827.log`

- 27/27 source registrations mapped to resolved WAV paths.
- The swap path at frame 1172 emitted `audio/swap.wav`.
- The following rejected swap at frame 1202 emitted `audio/bad.wav`.
- The guest PC trace still reaches `0x1801667c`, `0x18017e68`, and
  `0x18013878`; there were no fatal signatures.

This proves Bejeweled's guest source identity and host event routing for both
an accepted swap attempt and a rejected attempt. The current replay selected a
different deterministic board cell than the older `[8,6]` visual receipt, so
the combo/excellent trigger was not reproduced in this run. Match-specific
combo/excellent audio and physical mixer parity remain open.

## Headed sink replay

The same input was replayed through the headed desktop runner with the sink
enabled. Receipt: `/tmp/fliwheel_bejeweled_audio_headed_20260827.log`

- 27/27 source mappings were available to the frontend.
- 81 queued events produced 81 `played sound` receipts.
- The receipts include repeated `selector.wav`, `swap.wav`,
  `gotset.wav`, `gemongem.wav`, and `bad.wav` playback.
- The bounded process exited cleanly with no fatal, panic, or sink/decode
  error signatures.

`played sound` means the host decoder accepted the asset and the rodio sink
queued it. It does not yet prove the physical output waveform, volume, pan,
overlap, or timing matches an iPod.
