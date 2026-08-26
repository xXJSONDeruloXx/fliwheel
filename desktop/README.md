# fliwheel desktop runner

The desktop crate provides the native `fliwheel-eapp` runner for decrypted
iPod clickwheel game bundles. It supports the minifb display/input frontend,
headless regression runs, startup PPM capture, and optional desktop audio.

## Build

```sh
cargo build --release -p fliwheel-desktop --bin eapp
```

The binary is written to `target/release/eapp` for compatibility with the
repository scripts.

## Run a game

Pass a `Games_RO/<bundle-id>` directory, not the executable inside it:

```sh
CLICKY_EXPERIMENTAL_GL_HLE=1 \
CLICKY_GL_LIVE_CONTINUOUS=1 \
CLICKY_GL_PRESENT_VFLIP=1 \
target/release/eapp /path/to/Games_RO/66666
```

For reproducible headless checks:

```sh
target/release/eapp /path/to/Games_RO/66666 --headless --cycles 30000000
```

The `scripts/` directory contains per-title launchers and the decrypted-game
matrix harness. Runtime capture and trace options are documented in the
current reports under `docs/game_tests/`.
