# Sims normalized-coordinate casing fix

Date: 2026-08-27

## Finding

The shared live GL HLE already had the correct Sims projection formula, but it
compared the bundle ID against uppercase `1500C`/`1500E`. Runtime IDs are
normalized to lowercase during EAPP setup, so the projection was skipped. The
software rasterizer consequently received coordinates around `0..1` as if they
were pixels, producing zero or one-pixel coverage.

The matcher now uses a case-insensitive comparison for the two Sims IDs. The
change is shared by Bowling and Pool and does not add a title-specific asset or
completion shortcut.

## Regression

Focused unit test:

```bash
cargo test -p fliwheel-core --lib \
  eapp::live_gl::tests::ndc_detection_is_scoped_to_normalized_engine_bundles
```

Both titles were then run for 10,000,000 cycles with the live renderer and
continuous frame capture:

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_STARTUP_CAPTURE_DIR=/tmp/fliwheel_sims_ndcfix_20260827.<id>/captures \
target/release/eapp /path/to/Games_RO/<id> --headless --cycles 10000000
```

Observed results:

| Bundle | Exit | Capture rows | Unique hashes | Hash changes | Max draws | Zero-draw rows | First stable visual |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `1500C` Bowling | 0 | 20 | 5 | 5 | 2 | 2 | Coherent Sims Bowling title screen |
| `1500E` Pool | 0 | 20 | 4 | 4 | 2 | 1 | Coherent Sims Pool title screen |

The first stable Bowling title frame logged 606 covered pixels for the title
element and 76,800 for the full-screen composition. Pool logged 592 and 76,800
respectively. Captures from this run are in:

- `/tmp/fliwheel_sims_ndcfix_20260827.1500C/captures/`
- `/tmp/fliwheel_sims_ndcfix_20260827.1500E/captures/`

## Remaining state

This is a renderer milestone, not a playability claim. Bowling moves into a
one-draw partial follow-up state after the title screen; Pool has the same
incomplete follow-up pattern. Menu transition, gameplay, input, sound, and
persistence remain open.
