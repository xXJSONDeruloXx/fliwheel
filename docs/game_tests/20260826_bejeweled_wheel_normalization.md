# Bejeweled wheel position normalization

Date: 2026-08-26

This note records the wheel-scale correction for decrypted Bejeweled
(`55555`). It is an ABI correction that enabled the first deterministic
live-board match; it is not by itself a claim that headed play, every mode,
audio, or saves are complete.

## Why the physical value was wrong

The clickwheel hardware reports 96 detents. Bejeweled does not consume that
physical value directly:

- the title's input adapter at `0x1801d01c` copies the absolute wheel value
  from its input object at `+0x1c`;
- the shared packet path at `0x18020a08` masks the packet to its low byte and
  passes it through `0x1801237c`/`0x18014244` without a 96-detent conversion;
- the gameplay handler at `0x180136d0` divides the wheel into four sectors
  using boundaries around `0x21`, `0x61`, `0xa1`, and `0xe1`, over a 256-unit
  ring. Because the title's first navigation byte is the board-column axis,
the resulting sectors map to screen directions as follows:

| Guest angle | Guest field change | Screen direction |
| --- | --- | --- |
| `0x30` | `nav+0x709` increment | up |
| `0x70` | `nav+0x708` decrement | left |
| `0xb0` | `nav+0x709` decrement | down |
| `0xf0` | `nav+0x708` increment | right |

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
values and the tutorial cursor changing. A follow-up replay held the tutorial
completion input, entered the live board, selected `[8,6]`, and sent a
`0xf0` right-sector tap. The guest then entered the adjacent-swap path:

```text
frame 1172  pc=0x1801667c  nav+0x4: 2 -> 3
frame 1172  pc=0x18017e68  pair: [8,7] <-> [8,6]
frame 1172  pc=0x18013878  nav+0x4: 3 -> 4
```

The resulting capture shows “EXCELLENT!”, a changed/refilled board, and the
score-bar overlay. Receipt:

```text
/tmp/fliwheel_bejeweled_match_candidate_right_20260826.log
/tmp/fliwheel_bejeweled_match_candidate_right_20260826_capture/
```

This proves the guest-side target-gem selection, tap direction, swap, and
match-resolution path for this deterministic case. The remaining work is to
surface the same gesture through normal headed input and expand coverage.

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
