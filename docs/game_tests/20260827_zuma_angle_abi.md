# Zuma wheel-to-angle ABI probe

Date: 2026-08-27 UTC  
Bundle: `44444`

## Result

The generic clickwheel packet reaches Zuma's title code and is stored in both
of the guest's angle-related fields as a 16-bit fixed-point value. No HLE
mapping change is justified by this probe. The remaining aim gap is downstream
of packet delivery: the exact projectile/gameplay consumer has not yet been
isolated.

Static disassembly identifies the path as:

```text
InputEvents:0 return
  0x1802e1d4  mask packet to 8 bits
  0x1802e1d8  call 0x180100d8
  0x180100d8  call 0x180120b4
  0x180120b4  value << 16
  0x180120bc  store at 0x180bf898
  0x180120c4  store at 0x180bfaa0
```

The second store is `0x180bf898 + 0x208`, matching the relocated title
layout. The nearby pending-input object is copied by `0x180135e0`; that copy
reads the `+0x208` field at `0x180135e4`, but this is not yet proven to be the
projectile solver.

## Runtime evidence

The deterministic tutorial-to-board replay was stopped after guest frame
`2110` with a release build and no source probe enabled. The input script used
two controlled wheel windows:

```text
wheel=3:1850-1852
wheel=-6:2050-2052
```

The resulting `InputEvents:0` packets and watched writes were:

| Frames | Packet low byte | `0x180bf898` | `0x180bfaa0` |
| ---: | --- | --- | --- |
| `1850..1852` | `f0, e0, d0` | `00f00000, 00e00000, 00d00000` | same |
| `2050..2052` | `f0, 10, 30` | `00f00000, 00100000, 00300000` | same |

The writes are performed by guest PC `0x180120bc` and `0x180120c4`, with the
expected `value << 16` register values. The adjacent pending-input copy also
observed `0x00d00000` at PC `0x180135e4` during the first window.

Receipts:

```text
/tmp/fliwheel_zuma_angle_watch_20260827.log
/tmp/fliwheel_zuma_angle_reads_20260827.log
/tmp/fliwheel_zuma_input_dispatch_trace_20260827.log
```

## Interpretation

- The generic 8-bit packet contract is supported by the decrypted guest itself.
- The 96-detent-to-256-unit normalization reaches the guest in the expected
  form for both positive and negative scripted motion.
- The observed board/fire/result activity remains valid, but controlled shot
  direction is not yet proven.
- Do not add a Zuma-specific packet remap until the actual projectile consumer
  or a firmware/reference trace demonstrates a different scale or origin.
