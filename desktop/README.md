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

## Open the library

Run the binary without a bundle argument to open the native library launcher:

```bash
target/debug/eapp
```

It discovers `Games_RO` beside the checkout, the usual Downloads location, and
the preserved external corpus when present. Use `--library /path/to/Games_RO`
to add a source explicitly. In the library window, use Up/Down to select,
Enter or Space to play, A to choose a folder, R to rescan, Delete to forget a
source without deleting its files, and Escape to quit. Added sources are kept
in the platform application-support directory; `FLIWHEEL_CONFIG_DIR` can point
tests or portable setups at another configuration directory.

## Run a game

Pass a `Games_RO/<bundle-id>` directory, not the executable inside it, when a
direct launch is preferred:

```sh
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
target/release/eapp /path/to/Games_RO/66666
```

For reproducible headless checks:

```sh
target/release/eapp /path/to/Games_RO/66666 --headless --cycles 30000000
```

The `scripts/` directory contains per-title launchers and the decrypted-game
matrix harness. Runtime capture and trace options are documented in the
current reports under `docs/game_tests/`.

The game window maps arrow keys to the shared clickwheel side-button inputs,
Enter to Select, and M to Menu. For Bejeweled they also model the four
clickwheel touch quadrants used to swap the selected gem; scroll-wheel input
moves the selection. Escape closes the game window and returns to the library
when launched from there.
