# fliwheel

`fliwheel` is a clean working repository for progressively improving emulation of
decrypted iPod click-wheel games.

The initial code baseline is the decrypted-game HLE from the `clicky` fork. It
keeps the ARM interpreter, EAPP loader, OpenGLES emulation, live framebuffer
path, DMA overlays, input plumbing, game probes, and the existing per-game
research notes. The repository is deliberately separate so new experiments can
be measured against a known baseline without rewriting the original checkout.

## Current baseline

The starting point is clicky commit `d1d735973f404ca53cd3e4b9f6e4e3dcb38b4df1`.
The baseline runs decrypted `Games_RO/<bundle-id>` directories through the
experimental EAPP HLE. It is not yet a full RetailOS boot path and it does not
load encrypted `.ipg` packages.

The current compatibility assessment is recorded in
[`docs/game_tests/20260825_hle_baseline.md`](docs/game_tests/20260825_hle_baseline.md).

## Quick start

```sh
cargo build --release -p clicky-desktop --bin eapp
TIMEOUT_SECONDS=8 ./scripts/test_decrypted_games.sh /path/to/Games_RO
```

Run one bundle directly:

```sh
CLICKY_EXPERIMENTAL_GL_HLE=1 \
CLICKY_GL_GATE_B=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 \
CLICKY_GL_PRESENT_VFLIP=1 \
./target/release/eapp /path/to/Games_RO/66666 --headless
```

## Working model

- Decrypted game bundles and firmware stay outside the repository.
- SC Info, FairPlay keys, saves, and other identity-bearing material are never
  copied into this checkout.
- Every rendering or behavior change should have a reproducible run, a small
  hypothesis, and a note in `docs/` or the relevant game report.
- `clicky` crate names are retained in the first baseline to preserve parity;
  renaming is deferred until it can be done without obscuring regressions.

## Reference work

The sibling checkouts under `~/Developer` and their exact starting commits are
listed in [`docs/FLIWHEEL_BASELINE.md`](docs/FLIWHEEL_BASELINE.md). The
`siggifly/ipod-emulator` repository is the reference for the full 5.5G
firmware/ARM/RetailOS path. The `ipod-games` and DRM repositories provide
format, ABI, and authorization context; they are references, not vendored
dependencies.

