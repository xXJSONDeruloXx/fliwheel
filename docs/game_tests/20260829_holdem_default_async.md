# Hold'em default async and table-entry receipt

Date: 2026-08-29 UTC  
Repository: `fliwheel`  
Bundle: `33333` (`HoldEm_1_1_2563291.bin`)

## Result

The three Hold'em resource stages previously used only by the scoped oracle
probe are now enabled by default for this bundle. A clean no-override run
reaches the coherent green poker table after the name-entry route:

- exit code: `0`;
- CPU budget: `30,000,000` cycles;
- completed capture rows: `979` (last guest frame `978`);
- maximum draws: `111`;
- fatal signatures: none;
- target capture: guest frame `902`.

The frame-902 capture shows the green table, five player panels, blinds, pot,
local player name, and the table artwork. This establishes default resource
completion and table rendering. It does not yet establish a complete hand,
correct betting/action semantics, complete audio traversal, or persistence.

## Reproduction

```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_PRESENT_VFLIP=1 \
FLIWHEEL_EAPP_INPUT_SCRIPT='action:100-105,action:120-125,action:140-145,action:160-165,action:180-185,action:200-205,action:220-225,wheel=104:240-240,wheel=4:360-360,wheel=4:400-400,wheel=4:440-440,action:480-485,action:800-805,action:820-825,action:840-845,action:900-905,action:1000-1005' \
EAPP_AUDIO_DISABLE=1 \
FLIWHEEL_STARTUP_CAPTURE_DIR='/Volumes/NO NAME/fliwheel-runs-20260829/holdem-default-promoted-real/capture' \
FLIWHEEL_STARTUP_CAPTURE_PERIOD=1000 \
FLIWHEEL_STARTUP_CAPTURE_TARGET_FRAMES=496,503,800,900,902 \
FLIWHEEL_STARTUP_CAPTURE_MAX_FRAMES=1000 \
FLIWHEEL_STARTUP_CAPTURE_MAX_DUMPS=10 \
target/debug/eapp \
  '/Volumes/NO NAME/fliwheel-decrypted-corpus-20260826/20 iPod games/Games_RO/33333' \
  --headless --cycles 30000000
```

No `EAPP_TEXAS_ASYNC*` variables are present in this reproduction. For an A/B
diagnostic, set an individual stage to `0`; other titles do not inherit these
settings.

Evidence is stored on the external drive at:

```text
/Volumes/NO NAME/fliwheel-runs-20260829/holdem-default-promoted-real/
```

The earlier no-override baseline is retained at
`/Volumes/NO NAME/fliwheel-runs-20260829/holdem-default-baseline/`; it reached
the old 34-draw loading animation but not the table. The prior scoped route
and direct-oracle comparison remain documented in
`docs/games/33333_holdem.md`.
