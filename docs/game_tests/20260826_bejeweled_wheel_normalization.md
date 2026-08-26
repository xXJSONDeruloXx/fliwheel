# Bejeweled wheel position normalization

Date: 2026-08-26

This note records the wheel-scale correction for decrypted Bejeweled
(`55555`). It is an ABI correction, not a claim that the tutorial or the
full game is now playable.

## Why the physical value was wrong

The clickwheel hardware reports 96 detents. Bejeweled does not consume that
physical value directly:

- the title's input adapter at `0x1801d01c` copies the absolute wheel value
  from its input object at `+0x1c`;
- the shared packet path at `0x18020a08` masks the packet to its low byte and
  passes it through `0x1801237c`/`0x18014244` without a 96-detent conversion;
- the gameplay handler at `0x180136d0` divides the wheel into four sectors
  using boundaries around `0x21`, `0x61`, `0xa1`, and `0xe1`, over a 256-unit
  ring.

With a raw `0..95` packet, most of those sectors are unreachable. The direct
HLE implements the EAPP import boundary and skips the RetailOS layer that
would normally perform this conversion, so it now keeps the physical
position internally and emits:

```text
guest_position = floor(physical_position * 256 / 96)
packet = 0x40000000 | guest_position
```

Idle frames still emit no wheel packet, preserving the guest's touch/release
edge behavior.

## Bounded evidence

The pre-fix probe used the same script and delivered low-byte values
`2, 4, 6, ...`. After the correction:

```text
frame 840  bits=0x40000005  physical position=2
frame 841  bits=0x4000000a  physical position=4
frame 842  bits=0x40000010  physical position=6
...
frame 849  bits=0x40000035  physical position=20
```

The guest-side trace also shows the absolute field receiving those normalized
values and the tutorial cursor changing. The tutorial remains in its
“Selecting Gems” state (`nav+0x724 == 2`), so the next unresolved contract is
the target-gem selection/tap gesture rather than packet ingress.

Receipt:

```text
/tmp/fliwheel_bejeweled_wheel_normalized_probe_20260826.log
```

The unit and release-build gates passed:

```text
cargo test -p fliwheel-core wheel_packet -- --nocapture  # 2 passed
cargo build --release -p fliwheel-desktop --bin eapp     # passed
```

The bounded trace helper is opt-in. `FLIWHEEL_EAPP_STOP_FRAME=N` halts after
guest frame `N`; `FLIWHEEL_EAPP_PC_TRACE`, `FLIWHEEL_EAPP_PC_TRACE_LIMIT`, and
`FLIWHEEL_EAPP_PC_TRACE_DETAIL=1` add selected register/state observations.
