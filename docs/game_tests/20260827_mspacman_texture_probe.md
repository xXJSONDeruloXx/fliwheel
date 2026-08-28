# Ms. PAC-MAN texture-association probe

Date: 2026-08-27 UTC  
Bundle: `14004`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004`

## Result

The live renderer now keeps the launch image coherent. Before this change,
Ms. PAC-MAN uploaded its named font/UI/maze atlases into one reused guest
buffer, then submitted the loading scene through material `0x19` with no
nonzero texture bind. The dimension-only fallback consequently selected later
smaller atlases for the launch scene, producing stretched bars and glyph
fragments.

The narrow fallback in `core/src/sys/eapp/live_gl.rs` recognizes only bundle
`14004`, material `0x19`, and the untagged 512×256 upload captured before the
named resources. The 12 loading-scene draws then all select upload `0`, while
nonzero texture binds retain their normal precedence.

## Reproduction

```bash
env \
  FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
  FLIWHEEL_GL_GATE_B=1 \
  FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
  FLIWHEEL_GL_PRESENT_VFLIP=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC3_COMPLETE=1 \
  FLIWHEEL_EAPP_MSPACMAN_ASYNC0_COMPLETE=1 \
  FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_mspacman_async03_initialtex_20260827 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=120 \
  FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=120 \
  FLIWHEEL_EAPP_AUDIO_DISABLE=1 \
  ./target/release/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/14004' \
  --headless --cycles 12000000
```

Observed in `/tmp/fliwheel_mspacman_async03_initialtex_20260827/`:

- 120 captured frames with one stable post-load visual hash;
- all 12 draws rasterized, with the `0x19` draws selecting upload `0`;
- all 26 observed async resource requests completed under the opt-in probe,
  followed by the diagnostic `extra life.wav` callback;
- no fatal signature, menu, maze, or playable input state.

At the time of this texture-only probe, the async completion flags were still
diagnostic-only and completing the observed callbacks was not sufficient to
make the guest leave its loading state. The later post-resource investigation
reached the guest's menus and Stage 1, after which the four measured callback
contracts were promoted to the default path; see the gameplay probe for the
current result.
