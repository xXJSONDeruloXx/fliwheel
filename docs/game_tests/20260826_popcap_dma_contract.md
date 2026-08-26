# PopCap DMA/display contract probe

Date: 2026-08-26 UTC  
Titles: `44444` Zuma and `55555` Bejeweled  
Runner: `target/release/eapp` with the experimental live GL HLE  
Budget: 50,000,000 CPU cycles per title

This probe supersedes the older “hangs on DMA completion” description. Both
titles now complete the bounded run without a fatal memory error and perform a
full 320x240 DMA-buffer write. The remaining failure is visual/content
correctness, not an established missing status-bit response.

## Baseline evidence

| Title | Exit | Captured frames | GL max draws | DMA writes | DMA coverage | DMA non-zero pixels | Final presented hash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Bejeweled (`55555`) | 0 | 7 | 37 | 38,400 w32 | 153,600 bytes / 100% | 76,794 / 76,800 | `0x11339859df9232f9` |
| Zuma (`44444`) | 0 | 8 | 8 | 38,400 w32 | 153,600 bytes / 100% | 27,352 / 76,800 | `0x1700d27e0a94fea8` |

The final frames are now included in the startup manifest by the DMA-only
present path. The focused receipts are:

```text
/tmp/fliwheel_bejeweled_dma_capture_20260826/
/tmp/fliwheel_zuma_dma_capture_20260826/
```

The Bejeweled frame is a full-screen but malformed/warped scene with
recognizable title fragments. Zuma is a noisy partial top-of-screen scene with
the remainder black. Neither is a playable board.

## What the trace establishes

The guest first copies 0x20000 bytes of zeroes through `0x14000000`, then uses
the same copy helper at guest PC `0x1800172c/0x18001734` to write 0x25800 bytes
at `0x14020000..0x140457ff`. In that range, `0x1402000c` is the seventh pixel
word of the framebuffer write stream; it is not a completion register in this
run. The bounded read trace also showed a sequential read of the initial
0x14000000 aperture, but no title-specific completion poll was identified.

The current HLE therefore has two important facts and one unresolved model:

1. mapping the aperture keeps both games alive through the write;
2. the final buffer is large and populated, so a “DMA never completes” fix is
   not supported by this checkout;
3. the control/data semantics and PopCap composition contract are still not
   understood well enough to claim RGB565 display parity.

## SDRAM-alias A/B

The firmware reference model in `~/Developer/ipod-emulator` documents
`0x14000000..0x18000000` as an uncached SDRAM alias. An opt-in fliwheel probe
(`CLICKY_EAPP_UNCACHED_ALIAS=1`) mapped that window onto the eApp work RAM while
mirroring the observed DMA region for capture. Bejeweled produced the same
final presented hash, then exited with a fatal read at PC `0x00000008` from
`0x00000004` after the full frame was presented. This is negative A/B evidence:
the alias model did not improve the title and remains disabled by default.

## Next gate

Keep the final DMA receipts as the regression baseline. The next PopCap change
should be evidence-driven around the hardware aperture’s read/write contract
and the guest’s software-rendered buffer, then re-run both titles together.
Texture association and board composition must improve without masking the
existing GL draws or reintroducing the old fatal memory error.
