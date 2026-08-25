# Sudoku input ingress regression

Date: 2026-08-25

Bundle: `50513` (`Sudoku_1_1_2703081.bin`)

Corpus: `/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO`

## Purpose

The original no-input smoke test proved that Sudoku boots and renders its
splash/title content, but it could not exercise the menu-driven state machine.
This run checks the smallest host input contract needed to reach that state
machine without changing the default input behavior for the other games.

## Reproduction

```sh
CLICKY_EAPP_INPUT_SCRIPT='menu:30-40' \
  CLICKY_EXPERIMENTAL_GL_HLE=1 CLICKY_GL_GATE_B=1 \
  CLICKY_GL_LIVE_CONTINUOUS=1 CLICKY_GL_PRESENT_VFLIP=1 \
  CLICKY_STARTUP_PROGRESS_TRACE=1 \
  RUST_LOG='EAPP_INPUT=info,EAPP_PROGRESS=info,EAPP=warn,EAPP_IMPORT=warn' \
  timeout 3s target/release/eapp \
    '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO/50513' \
    --headless
```

Exit `124` from the watchdog is expected. The relevant run log is
`/tmp/fliwheel_sudoku_input/sudoku_menu_final.log`.

## Result

The `InputEvents:0` import now accepts both owner-register shapes observed in
the decrypted titles:

| Title shape | `r4` | `r5` | Owner used |
| --- | --- | --- | --- |
| Tetris-family call | work-RAM object | context | `r4` |
| Sudoku call | empty | `0x10500b00` work-RAM object | `r5` |

At frame 30 the HLE writes a Menu press event (`id=1`, `kind=2`) to Sudoku’s
event list at `0x10502b80`, and links it through owner `0x10500b00`. The guest
reaches `frame_state=6` at frame 433, invokes the save-file path
(`AsyncFileIO:12/14/16`), then returns to `frame_state=4`. The run remains
clean through the watchdog: no fatal memory fault, panic, or emulator crash.

The event list is not cleared by the host for this reversed-register family.
An experiment that cleared it during the raw input poll made the guest miss
the queued Menu event; the change was removed. Tetris’s existing owner shape
still clears its press/release transition normally after the guest observes it.

## Boundary

This verifies menu ingress, not complete playability. The current evidence does
not yet show the correct action sequence for entering a Sudoku board or moving
the cursor. Scripted raw packet values `0x40000001` through `0x40000005` also
did not advance the visible state, so they remain an open hardware-packet
mapping question rather than a claimed fix.
