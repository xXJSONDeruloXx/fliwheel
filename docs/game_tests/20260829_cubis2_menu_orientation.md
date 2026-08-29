# Cubis 2 menu orientation probe

Date: 2026-08-29  
Bundle: `99999`  
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/99999`

## Result

The current live GL HLE reaches Cubis 2's first complete menu after the
resource-loading phase. Guest frame 64 has 49 draws and a stable presented hash
of `0xe9107d570d52b01f`. The composition contains the background, logo,
selection ornaments, menu labels, and the 16-pixel font atlas; the run exits
cleanly after 5,000,000 cycles with no fatal signature.

The first capture was intentionally run with
`FLIWHEEL_GL_PRESENT_VFLIP=1`. Its background and art were present, but the
menu labels were upside down. A vertical inspection flip made the labels
readable, showing that Cubis 2 belongs with the PopCap-style screen origin. The
automatic default now selects that orientation for bundle `99999`, so normal
launches do not need the override.

## Reproduction

```bash
RUN_ROOT='/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-menu-current-default-20260829'
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_STARTUP_CAPTURE_DIR="$RUN_ROOT/capture" \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1000 \
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=120 \
FLIWHEEL_STARTUP_CAPTURE_TARGET_FRAMES='64,65,70,80' \
target/debug/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/99999' \
  --headless --cycles 5000000
```

The explicit orientation override is useful for reproducing the before/after
comparison, but is not part of the corrected command. The preserved evidence
directory is:

`/Volumes/NO NAME/fliwheel-runs-20260829/cubis2-menu-current-20260829/`

## Boundary

This is a renderer/orientation milestone, not a complete Cubis 2 acceptance
claim. The next title-specific work is to drive New Game into an isometric
board, validate wheel/Select cube launching and matching, exercise pause/return
and save behavior, and map the title's music/effects through the desktop sink.

Reference material used for the visual target includes the
[MobyGames Cubis 2 screenshots](https://www.mobygames.com/game/24363/cubis-2/screenshots/)
and the preservation project's
[Cubis 2 gameplay reference](https://www.youtube.com/shorts/SxvEaaQP94E).
