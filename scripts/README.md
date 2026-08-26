# Test and launch scripts

These scripts exercise the decrypted `Games_RO` bundles through the native
EAPP runner.

- `test_decrypted_games.sh` runs a short headless smoke pass over every bundle.
- `test_decrypted_games_interactive.sh` runs scripted wheel/button input and
  records captures, hashes, audio events, and fatal signatures.
- The per-game launchers (`tetris.sh`, `zuma.sh`, `bejeweled.sh`, and the other
  title scripts) provide headed runs with optional captures and timeouts.
- `games/ipod_games_probe.py games /path/to/Games_RO` inventories manifests,
  EAPP headers, imports, and referenced asset paths.

All launchers accept a bundle path or a title-specific `*_BUNDLE` variable.
The renderer switches retain their historical `CLICKY_*` names for command-line
compatibility; they are documented in the root README and game reports.
