# Test and launch scripts

These scripts exercise decrypted `Games_RO` bundles through the native EAPP
runner.

## Corpus probes

- `test_decrypted_games.sh` runs a short headless smoke pass over every bundle.
- `test_decrypted_games_interactive.sh` runs scripted wheel/button input and
  records captures, hashes, audio events, and fatal signatures.
- `games/ipod_games_probe.py` inventories manifests, EAPP headers, imports, and
  referenced asset paths.

## Launching one title

Every decrypted bundle has a small entrypoint under `scripts/games/`:

```sh
./scripts/games/tetris.sh --headless
./scripts/games/bejeweled.sh --timeout 15
./scripts/games/iquiz.sh --no-build --dump 30
```

All entrypoints use the shared `run_game.sh` implementation and accept:

```text
--bundle PATH       override the bundle directory
--no-build          skip the cargo build
--no-capture        skip PPM captures
--headless          run without a window
--verbose           enable debug logging
--timeout SECONDS   terminate after a bounded interval
--dump COUNT        dump the first COUNT frames as PPM
--log-level LEVEL   override RUST_LOG
--                  pass remaining arguments to eapp
```

The historical `CLICKY_*` environment names remain accepted by the runner for
compatibility with existing experiments. Title-specific defaults, including
PopCap screen origin and normalized-coordinate presentation, are selected
centrally in `run_game.sh` and the HLE.
