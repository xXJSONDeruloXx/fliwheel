# Sudoku puzzle board and input milestone

Date: 2026-08-27 UTC  \
Bundle: `50513`  \
Repository: `fliwheel`  \
Corpus: `/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO`

This receipt extends the title-scoped RLB/resource experiment recorded in
[`20260827_sudoku_rlb_seek_and_setup.md`](20260827_sudoku_rlb_seek_and_setup.md).
It does not change the default cross-title async contract.

## Verified route

The following input sequence now reaches the first real puzzle scene:

```text
action:8200-8201
action:8500-8501
action:9450-9510
wheel=-4:9700-9701
wheel=-4:9750-9751
wheel=-4:9800-9801
action:9900-9901
action:10300-10301
```

The first two actions enter the name/setup flow. The long Center hold accepts
the current name character. Three single-item wheel moves select the name
checkmark, and Center validates the name. The next Center action dismisses the
tutorial and exposes the puzzle board.

## Visual evidence

The bounded live-GL replay used:

```text
EAPP_SUDOKU_ASYNC0_COMPLETE=1
EAPP_SUDOKU_ASYNC1_COMPLETE=1
EAPP_SUDOKU_ASYNC2_COMPLETE=1
FLIWHEEL_EAPP_ASYNC0_RESULT=length
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
FLIWHEEL_STARTUP_CAPTURE_PERIOD=10
FLIWHEEL_STARTUP_CAPTURE_DUMP_START_FRAME=10400
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=11200
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=100
./target/release/eapp .../Games_RO/50513 --headless --cycles 70000000
```

The resulting captures are retained at:

```text
/tmp/fliwheel_sudoku_boardprobe.Z5B47q/capture/manifest.tsv
/tmp/fliwheel_sudoku_digitprobe.GPFW4K/capture/manifest.tsv
/tmp/fliwheel_sudoku_validdigit7.fliwheel_sudoku_validdigit7.MYUBCq/capture/manifest.tsv
```

Observed scenes:

- the built-in `TUTORIAL` screen;
- a populated 9×9 clue board with the side controls and red cell cursor;
- Center opens the numbered entry palette with `1` selected;
- a larger wheel packet moves the palette highlight, confirming that the
  palette is an active consumer rather than a static texture.

The shared half-texel containment fix produces both board-side full-surface
draws without a skipped-draw diagnostic. The bounded replay exits with status
0 and retains the same late invalid-surface loop after the captured input
window.

## What is not yet proven

The first entry probe selected `1`, which is illegal for the highlighted cell,
so the board correctly remained unchanged. A larger wheel packet selected `4`,
which is also illegal for that cell; a small packet was below the title's
selection threshold. Therefore the following remain open:

- calibrated legal digit selection and cell-entry confirmation;
- complete board cursor movement and the game's pen-mode toggle;
- error-checking, completion/win behavior, sound mixing, and persistence;
- the default (non-experimental) RLB completion path;
- the post-input invalid-surface loop.

Sudoku has crossed from setup-only to a genuine puzzle-board milestone, but it
is not fully playable or near-perfect.
