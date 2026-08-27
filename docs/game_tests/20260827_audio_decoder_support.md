# Desktop AAC audio support

Date: 2026-08-27

The desktop EAPP runner now compiles rodio's Symphonia AAC and ISO/MP4
backends and accepts the container extensions used by the decrypted corpus:
`.aac`, `.m4a`, `.m4b`, `.m4p`, `.m4r`, and `.mp4`, in addition to the existing
WAV/MP3/FLAC/OGG formats.

## Corpus evidence

The current decrypted corpus contains 103 non-AppleDouble `.m4a` assets. The
sample assets below identify as stereo AAC at 44.1 kHz in an ISO/MP4 container:

```text
11050/1.m4a   AAC, 44100 Hz, 2 channels, 118.212789 s
12345/a.m4a   AAC, 44100 Hz, 2 channels,  57.260408 s
```

The extension filter is covered by the desktop binary unit test. This change
only removes the frontend decoder rejection; core-side resource-to-sound
event mapping remains title-specific and is not yet claimed for the games
whose audio ABI has not been derived.

## Verification

```bash
cargo test -p fliwheel-desktop --bin eapp
cargo test -p fliwheel-core --lib eapp
```
