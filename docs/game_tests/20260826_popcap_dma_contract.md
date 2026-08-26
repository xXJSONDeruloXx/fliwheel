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

## Raw-buffer provenance

The runner can now preserve the exact completed DMA payload before the HLE
composites it into a presented frame:

```text
FLIWHEEL_EAPP_DMA_DUMP_DIR=/tmp/fliwheel_popcap_raw \
  target/release/eapp '<game-dir>' --headless --cycles 50000000
```

Each `dma_frame_NNNNNN.rgb565` file is 153,600 bytes. The 2026-08-26 raw
receipts are:

```text
/tmp/fliwheel_bejeweled_dma_raw_20260826/dma_frame_000007.rgb565
/tmp/fliwheel_zuma_dma_raw_20260826/dma_frame_000008.rgb565
```

The native little-endian RGB565 interpretation is materially more plausible
than byte-swapped variants, but Bejeweled still has a geometrically wrapped
title/background and Zuma remains a partial/noisy buffer. A diagnostic
half-width row rotation improves Bejeweled's title alignment, yet does not
repair the rest of the scene and is not enabled: the firmware reference's
ordinary 320x240 RGB565 path is linear, so this visual clue is not sufficient
evidence for a hardware presentation transform.

For source/destination attribution, add the bounded hardware trace and register
state:

```text
FLIWHEEL_EAPP_HW_TRACE=32780 FLIWHEEL_EAPP_HW_TRACE_REGS=1 \
  target/release/eapp '<game-dir>' --headless --cycles 50000000
```

The first real Bejeweled pixel write is guest-authored: `r0=0x14020000` is the
aperture destination and `r1=0x13d9df8c` is the work-RAM source. The earlier
zero-fill uses the same transfer shape with destination `0x14000000`. This
rules out treating `0x1402000c` as a completion register and keeps the next
investigation focused on the guest-produced buffer and aperture semantics.

## Control-read A/B

The synthetic control aperture historically returned `1` for every read. The
opt-in `FLIWHEEL_EAPP_HW_CONTROL_VALUE=<value>` override makes that value
measurable without changing framebuffer writes. A paired 50,000,000-cycle run
with the value forced to `0` produced the same result as the default for both
titles:

| Title | Default raw SHA-256 | Control=0 raw SHA-256 | Result |
| --- | --- | --- | --- |
| Bejeweled (`55555`) | `f6f31a7874104f7b26479a71a150083b0ba469a22faf1e52ae9c3180a057a3df` | identical | full write, exit 0 |
| Zuma (`44444`) | `b84eb7639995bc2d1305e7417cfd04823e851de3ec9c2545d7b3ff351005a7c4` | identical | full write, exit 0 |

The corresponding raw files are under:

```text
/tmp/fliwheel_bejeweled_hwcontrol0_raw_20260826/
/tmp/fliwheel_bejeweled_hwcontrol1_raw_20260826/
/tmp/fliwheel_zuma_hwcontrol0_raw_20260826/
/tmp/fliwheel_zuma_hwcontrol1_raw_20260826/
```

This is negative evidence against the current control-read value being the
source of the PopCap visual corruption. The override remains diagnostic only;
the default remains `1`.

## Source-buffer A/B

The first aperture store occurs inside the optimized ARM copy after its first
16-byte load, so `r1 - 16` is the start of the source buffer at that store.
`FLIWHEEL_EAPP_DMA_SOURCE_DUMP_DIR=<dir>` preserves that work-RAM range before
the HLE writes the aperture. The paired source and DMA payloads are identical:

| Title | Source address | Source SHA-256 | DMA SHA-256 |
| --- | --- | --- | --- |
| Bejeweled (`55555`) | `0x13d9df7c` | `f6f31a7874104f7b26479a71a150083b0ba469a22faf1e52ae9c3180a057a3df` | identical |
| Zuma (`44444`) | `0x13e99fbc` | `b84eb7639995bc2d1305e7417cfd04823e851de3ec9c2545d7b3ff351005a7c4` | identical |

The raw source receipts are under:

```text
/tmp/fliwheel_bejeweled_source_probe_src_20260826/
/tmp/fliwheel_zuma_source_probe_src_20260826/
```

This moves the fault boundary upstream: the observed wrap/partial scene is
present in the guest-produced work-RAM buffer before the synthetic aperture is
visited. The next PopCap gate should inspect the software-renderer composition
and its guest-visible memory contract, rather than changing RGB565 byte order
or aperture write packing.

## Surface-copy chain

The source buffer is part of a repeated PopCap surface copy chain. An opt-in
watchpoint records the guest registers at the first write into a surface and
can dump the source range one copy earlier:

```text
FLIWHEEL_EAPP_WATCH=0x13d9df70,0x2580 \
FLIWHEEL_EAPP_WATCH_REGS=1 \
FLIWHEEL_EAPP_WATCH_SOURCE_DUMP_DIR=/tmp/fliwheel_popcap_watch \
target/release/eapp '<game-dir>' --headless --cycles 50000000
```

The first copy observed for each title has the same 12-byte surface-header
shape:

| Title | Destination surface base | Source payload | Next upstream copy |
| --- | --- | --- | --- |
| Bejeweled (`55555`) | `0x13d9df70` | `0x13b1beec` | `0x13b1bee0 <- 0x13899e5c` |
| Zuma (`44444`) | `0x13d9df70` | `0x13c17f2c` | `0x13b1bee0 <- 0x13995e9c` |

Following additional hops preserves the same relationship: a copy reads
from the previous surface base plus `0x0c` and writes the next surface base.
The first aperture source likewise starts at `surface_base + 0x0c`. The
diagnostic `dma_surface_*.rgb565` dump includes the 12-byte prefix; for both
titles, its bytes at offset `0x0c` are byte-for-byte identical to the captured
DMA frame. This makes the aperture copy and the 12-byte payload offset
internally consistent, while confirming that the malformed geometry is already
present in the software-rendered surface.

The observed PCs disassemble to the ordinary ARM `LDMIA`/`STMIA` forward-copy
loop, with title-specific offsets in the two binaries. The current block
transfer implementation uses aligned ascending register-list semantics for
these copies, and no evidence justifies changing it or adding a presentation
rotation. The next useful target is the surface creation/render contract that
produces the first malformed surface, not the final DMA store.

## File-backed source boundary

The backward walk reaches a more useful boundary in Bejeweled than another
heap surface. Watching the low staging destination `0x1016d1f0` catches the
resource-loader copy itself:

| Field | Observed value |
| --- | --- |
| First store PC | `0x180016cc` |
| Resource-loader copy call | `0x1801e108` (`lr=0x1801e10c`) |
| Destination | `0x1016d1f0` |
| Source at first store | `0x180ad8d0` |
| Source dump start (`r1 - 16`) | `0x180ad8c0` / executable offset `0xad8c0` |
| Requested copy length | `0x10bac` / 68,524 bytes |

The source is file-backed eApp memory, not work RAM. The new diagnostic source
dump preserves both cases, so the receipt from this run is:

```text
/tmp/fliwheel_bejeweled_file_source_20260826/watch_source_0x180ad8c0.bin
```

Its first 68,524 bytes match the executable slice at offset `0xad8c0`
byte-for-byte. Both slices have SHA-256
`1b33c57a425af115dad57ed71dbb6bfe9b6277c4c296bced137be92a339ff274`.
The full 153,600-byte diagnostic dump has SHA-256
`5fead233658063011ff8b90b12dc6ad883c8c277d81a554f2b0ea1d8736cfa1e`.

This proves that the file-backed guest mapping and the resource-loader copy
are reproducing the embedded asset bytes. The bytes decode as a coherent
PopCap title/background surface, but the correct resource dimensions, pitch,
and first composition operation are still unresolved. The current visual
failure therefore cannot yet be assigned to the DMA aperture, the file read,
or the ARM copy loop.

## AsyncFileIO:3 byte-count contract

The next boundary was the request completion object, not the DMA aperture.
The callback at `0x1801fe8c` loads request words `+0x20` (status) and `+0x24`
(byte count) before forwarding both values to the owner completion helper.
The HLE was copying the `.ro` payload into the requested destination but was
leaving those two words at zero for PopCap.

That zero is not harmless. Bejeweled's resource object is initialized with
`[object+0xc] = 0` because the observed async request used the actual-file
length only in the HLE, not in the guest completion fields. The final resource
table entry then computes its length as `0 - offset`. For game 1, the measured
values are:

```text
actual async read       464,304 = 0x71670
resource final offset   385,436 = 0x5e79c
malformed copy length    68,524 = 0x10bac
```

The `0x10bac` copy is exactly the low staging copy observed from the
file-backed executable range. This connects the missing callback byte count to
the malformed resource path with an arithmetic check, rather than a visual
guess.

The HLE now reports `(status=0, byte_count=actual_read)` for PopCap `.ro`
requests by default. It remains scoped to those resource reads, so audio and
preference requests keep their separately investigated contracts. A paired
30,000,000-cycle regression with no opt-in environment variable produced:

| Title | `.ro` reads | GL result after count fix | DMA result | Exit/fatal |
| --- | --- | --- | --- | --- |
| Bejeweled (`55555`) | `game1.ro` 464,304 B; `game2.ro` 448,148 B | resource-backed scene, 174 steady draws / 176 peak | no DMA framebuffer writes | 0 / none |
| Zuma (`44444`) | `title.ro` 332,284 B; `game.ro` 500,672 B; `graphictext_enUS.ro` 63,224 B | resource-backed scene, 11 steady draws | no DMA framebuffer writes | 0 / none |

The corrected path is materially further along: it reaches the title-specific
GL resources instead of ending in the earlier DMA-only composition. It is not
yet correct or playable. Bejeweled's later frame contains the expected
background/UI composition but garbled text and missing associations; Zuma's
scene is still sparse and incomplete. The next visual gate is therefore
texture association, texture orientation/format validation, and the PopCap
draw transform, not another synthetic completion-register or DMA-status
change. The paired receipts are:

```text
/tmp/fliwheel_bejeweled_startup_default_20260826/
/tmp/fliwheel_zuma_startup_default_20260826/
```

## PopCap near-surface full-screen draw

The first Zuma draw after the corrected resource loads uses material handle
`0x16` with a 320x240 screen-space quad and texel-centered UVs from
`(1,1)` through `(321,241)`. The decoded board upload is 322x222 RGBA4444,
so the existing full-containment selector rejected it because the submitted
V extent is 19 pixels taller than the upload. This was a selector problem,
not a short or malformed texture: the raw upload is a valid stone-framed
board surface.

The live GL selector now has a title- and material-scoped near-surface
fallback for PopCap handles `0x16`. It accepts the observed 320x240 span,
chooses the closest decoded upload large enough for the framebuffer, and
prefers RGB565 when a title also has a screen-sized RGB565 surface. The
fallback is covered by the live GL unit test and is not used by other game
families.

A fresh 8,000,000-cycle Zuma capture confirmed the change:

| Check | Result |
| --- | --- |
| First full-screen draw | upload `3`, 322x222 RGBA4444 |
| Raster coverage | 76,800 / 76,800 framebuffer pixels |
| Presented scene | complete stone-framed board visible |
| Run result | exit 0, no fatal signature, no DMA framebuffer writes |

The receipt is `/tmp/fliwheel_zuma_surface_20260826/`. The board is now a
useful regression boundary. The remaining defect is in the title/menu overlay
composition: text and decorative elements are still sparse, displaced, or
associated with the wrong atlas region. The next step is to trace those
overlay material/UV pairs and then drive a board-entry input sequence.

## SDRAM-alias A/B

The firmware reference model in `~/Developer/ipod-emulator` documents
`0x14000000..0x18000000` as an uncached SDRAM alias. An opt-in fliwheel probe
(`FLIWHEEL_EAPP_UNCACHED_ALIAS=1`) mapped that window onto the eApp work RAM while
mirroring the observed DMA region for capture. Bejeweled produced the same
final presented hash, then exited with a fatal read at PC `0x00000008` from
`0x00000004` after the full frame was presented. This is negative A/B evidence:
the alias model did not improve the title and remains disabled by default.

## Next gate

Keep the DMA receipts as historical boundary evidence and retain them as a
regression check. The active PopCap path is now the byte-count-corrected
resource-backed GL path. The next change should identify which uploaded
resource each material handle owns, validate the RGBA4444 row/orientation
contract, and follow the first board-composition operation. Any shared
renderer change must be paired against both titles without hiding GL draws or
reintroducing the old fatal memory error.

## Bound texture selection and screen origin

The next focused 30,000,000-cycle run used the interactive probe with the
title-aware orientation default. Both bundles exited with code 0 and no fatal
signature. Zuma's frame-8 draw details now preserve the guest's live
OpenGLES:4 texture bind instead of inferring the sampled image from the shared
material handle:

| Draw | Material | Bound texture | Upload | Size | Role |
| ---: | ---: | ---: | ---: | ---: | --- |
| 2 | `0x10` | `7` | `7` | 202x44 RGBA4444 | small title/menu strip |
| 3-9 | `0x10` | `6` | `6` | 488x135 RGBA4444 | board/menu overlay atlas |
| 10-11 | `0x10` | `5` | `5` | 510x212 RGBA4444 | text/decorative atlas |

The material handle `0x10` is reused across these unrelated atlases. Using
the live bind removes that ambiguity; it is covered by a unit test that gives
two overlapping UV ranges different bound texture names. The focused receipts
are:

```text
/tmp/fliwheel_popcap_boundtex_20260826/
/tmp/fliwheel_popcap_default_20260826/
```

The corresponding upright Zuma capture shows the stone-framed board,
`PRESS SELECT TO ENTER THE TEMPLE`, `STAGE 1`, and `TEMPLE OF ZUKULKAN`.
This is a verified renderer boundary, not a playable-game claim: the marble
track, launcher interaction, and some overlay artwork remain incomplete.
Bejeweled's focused filesystem regression now reaches its board and built-in
tutorial; its selector interaction remains incomplete.

The orientation A/B control run with `FLIWHEEL_GL_PRESENT_VFLIP=0` produced the
same upright Zuma screen. The HLE therefore defaults PopCap bundles `44444`
and `55555` to the guest screen origin; the environment variable remains
available for explicit comparisons. Visual references used to sanity-check
the expected upright device presentation include [a Bejeweled iPod image](https://www.cultofmac.com/news/an-illustrated-history-of-the-ipod-and-its-massive-impact-ipod-10th-anniversary),
[a Zuma-like iPod preservation image](https://i.blogs.es/2aae01/juegos-ipod/840_560.jpeg),
[a Zuma review video](https://www.youtube.com/watch?v=jU-YUGcqtMU), and
[the Bejeweled iPod reference page](https://bejeweled.wiki.gg/wiki/Bejeweled_%28iPod%29).

## Manifest-title default regression

The automatic orientation selector receives the manifest title from the EAPP
loader, not the numeric bundle directory name. The selector now recognizes
both forms, so PopCap's guest screen origin is selected when
`FLIWHEEL_GL_PRESENT_VFLIP` is unset. The post-fix 30,000,000-cycle pair was:

```text
/tmp/fliwheel_reorg_popcap_20260826/interactive_matrix.md
```

Both titles logged `present_vflip=false`, exited with code `0`, and had no
fatal signature. The run produced 93 unique hashes for Zuma and 69 for
Bejeweled; this is a default-selection regression, not a new gameplay claim.
