# Mahjong RLB stream and texture-format probe

Date: 2026-08-29  
Bundle: `77777`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/77777`

## Result

The Mahjong resource path now follows the measured stream contract from the
reference runner:

- the bufferless `main.rlb` open publishes a synthetic stream handle through
  the file object's `+0` field and uses a zero operation result at `+8`;
- op-5 seek and op-3/op-4 reads use the staged RLB with the guest callback
  shape `(request, resource-context)`, where
  `resource-context + 0x128 == request`;
- resource token `0x8808` decodes as RGBA5551 and `0x0801` as RGB565.

That last mapping is confirmed by the reference runner's corresponding
`OpenGLES:99` calls (`GL_RGBA` + `GL_UNSIGNED_SHORT_5_5_5_1`, and `GL_RGB` +
`GL_UNSIGNED_SHORT_5_6_5`). The fliwheel capture now shows the readable EA
badge, copyright line, title logo, loading bar, and lower silhouette instead
of the former dotted/garbled output.

## Reproduction

```bash
RUN_ROOT='/Volumes/NO NAME/fliwheel-runs-20260829/mahjong-render5551-20260829'
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_MAHJONG_ASYNC0_COMPLETE=1 \
FLIWHEEL_MAHJONG_ASYNC2_COMPLETE=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_STARTUP_CAPTURE_DIR="$RUN_ROOT/captures" \
FLIWHEEL_STARTUP_CAPTURE_TARGET_FRAMES='6,7,8,9,10,16,32,64,96' \
target/debug/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/77777' \
  --headless --cycles 3000000 \
  > "$RUN_ROOT/run.log" 2>&1
```

The fliwheel receipt and captures are retained on the external drive at
`/Volumes/NO NAME/fliwheel-runs-20260829/mahjong-render5551-20260829/`.
The PR-3 reference receipt is at
`/Volumes/NO NAME/fliwheel-runs-20260829/mahjong-pr3-zero-result-20260829/`;
its texture dumps are `/tmp/ipod-tex-01.png` and `/tmp/ipod-tex-02.png` from
the reproducible reference command.

## Boundary

This is a title-rendering and file-ABI milestone, not a full-playability
claim. The opt-in flags still need to reach the main menu and a complete tile
matching session, then exercise sound, pause/return, save, and persistence.
The current title page keeps those flags opt-in until those gates have receipts.

The broader visual/control references include the
[iPod clickwheel-games catalog](https://ipodwiki.com/wiki/Clickwheel_games)
and the PR-3
[EAPP runner documentation](https://github.com/siggifly/ipod-emulator/pull/3).
