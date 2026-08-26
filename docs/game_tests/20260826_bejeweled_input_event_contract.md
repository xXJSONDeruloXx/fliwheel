# Bejeweled input event contract

Date: 2026-08-26

This probe isolates the linked `InputEvents` edge list from the compact
button packet for bundle `55555`. It keeps the shared mapping evidence-backed
while the remaining Bejeweled cursor/gameplay work is investigated.

## Observed mapping

The decrypted Bejeweled input adapter and menu transitions establish the
five event-list IDs used by the shared runtime:

| Event ID | Host control | Observed title behavior |
|---:|---|---|
| 1 | Menu / Back | Opens `EXIT BEJEWELED?` from the startup menu |
| 2 | Action / Select | Starts `Start Classic Game` and enters the loading path |
| 3 | Left | Reserved for the left side-button edge |
| 4 | Right | Reserved for the right side-button edge |
| 5 | Up / Down | Reserved for the vertical side-button edge |

The compact packet remains a separate path. Holding `bits=0x10` without an
event-list edge did not select `Start Classic Game`, so the HLE must continue
to provide both interfaces rather than treating the packet bit as a substitute
for the linked event node.

## Reproduction

The event-only startup checks used the current release runner and the
decrypted bundle:

```bash
EAPP=target/release/eapp
BUNDLE='/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/55555'

FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_DUMP_FRAMES=40 \
FLIWHEEL_EAPP_INPUT_SCRIPT='event=1:18-20' \
RUST_LOG='EAPP_GL=warn,EAPP=warn,EAPP_IMPORT=warn' \
"$EAPP" "$BUNDLE" --headless --cycles 12000000

FLIWHEEL_EXPERIMENTAL_GL_HLE=1 \
FLIWHEEL_GL_GATE_B=1 \
FLIWHEEL_GL_LIVE_CONTINUOUS=1 \
FLIWHEEL_GL_DUMP_FRAMES=40 \
FLIWHEEL_EAPP_INPUT_SCRIPT='event=2:18-20' \
RUST_LOG='EAPP_GL=warn,EAPP=warn,EAPP_IMPORT=warn' \
"$EAPP" "$BUNDLE" --headless --cycles 12000000
```

The resulting frame dumps were retained in:

```text
/tmp/fliwheel_bejeweled_event1_startup_frames.FHtBI9/
/tmp/fliwheel_bejeweled_event2_startup_frames.A3vD2C/
/tmp/fliwheel_bejeweled_raw_action_only_20260826.log
```

The binary-side path is the title adapter at `0x1801d01c`, which consumes the
translated event state after the shared event processor at `0x18013e6c`.
The current Rust mapping is covered by
`input_event_ids_follow_guest_button_mapping`.

## Boundary

This resolves an earlier false lead that proposed swapping the action and menu
IDs. It does not yet prove that the cursor can select adjacent gems, that
swaps resolve correctly, or that title audio and save persistence match the
device. The next Bejeweled probe should trace wheel-derived cursor state after
the tutorial's selection step.
