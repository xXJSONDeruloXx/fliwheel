# Pointer-Backed Material Draws — ABI Analysis

## Summary

Two material handles (`0x100e38e0`, `0x100e5260`) are **guest-RAM pointers**, not
small integer material IDs. They drive the skipped draws 9–14 and 21–29 in the
29-draw menu frame. This document records the evidence-backed object layout and
isolates the **next missing primitive** required to render them correctly.

---

## 1. Pointer Handle Object Layout

### Discovery: handles point to EMPTY allocation buffers

Dumping 0x40 words (256 bytes) at each pointer handle address:

```
ptr_handle_object handle=0x100e38e0 addr=0x100e38e0 words=[all zeros × 64]
ptr_handle_object handle=0x100e5260 addr=0x100e5260 words=[all zeros × 64]
```

**Both handles point to zero-initialized scratch memory.** The pointer is not a
material object — it is an opaque token / allocation ID. The real material data
lives in the **state pointer** passed as `r1` at ordinal 159.

### Material state block layout (at `state_ptr`)

```
offset  0x00: vtable / setup function pointer (image memory)
                 0x100e38e0 state: 0x18023e24
                 0x100e5260 state: 0x18023e24  (same setup function)
offset  0x04: 0x00000001  (flags / mode)
offset  0x08: 0x00000000
offset  0x0c: 0x00000000
offset  0x10: 0x00000000
offset  0x14: 0x3f800000  (1.0f — scale or constant)
offset  0x18: 0x00000000
offset  0x1c: 0x41400000  (12.0f)   ← 0x100e38e0  (glyph cell height)
                 0x41800000  (16.0f)   ← 0x100e5260
offset  0x20: 0x00000000
offset  0x24: 0x3f800000  (1.0f)
offset  0x28: 0x41200000  (10.0f)   ← 0x100e38e0  (glyph cell width)
                 0x41800000  (16.0f)   ← 0x100e5260
offset  0x2c: 0x41400000  (12.0f)   ← 0x100e38e0
                 0x41800000  (16.0f)   ← 0x100e5260
```

The float pairs (10.0, 12.0) and (16.0, 16.0) match the **glyph cell dimensions**
observed in the position array bounds. This is a **texture matrix / texgen
parameter block**, not a simple color material.

---

## 2. Ordinal 148 — Font/Glyph Descriptor Configuration

Ordinal 148 is called immediately before pointer-backed menu draws with:
```
r0 = 4
r1 = 1
r2 = pointer to descriptor struct (work RAM)
r3 = 0
```

### Descriptor struct layout (at `r2`)

```
offset 0x00..0x0c: 4× fixed-point tile sizes
                    e.g. [1.0, 1.0, 1.0, 0.3]  (0x00010000, 0x00010000,
                                                  0x00010000, 0x00004ccc)
offset 0x10..0x1c: 4× float scale multipliers (all 1.0f = 0x3f800000)
offset 0x20:       0x01010001  (flags: format=1, type=1, ...)
offset 0x24..0x30: [1, 0, 1, 0]  (counts / indices)
offset 0x34..0x4c: 7× work-RAM pointers (glyph matrix tables)
                    ptr[0] at +0x34, ptr[1] at +0x38, ... ptr[6] at +0x4c
```

### Glyph matrix tables (the 7 sub-pointers)

Each of the 7 pointers targets a 16-word block that looks like **matrix rows**
(interleaved with zeros — likely IEEE-float 4×4 matrices stored sparsely):

```
slot 13: [fontObjPtr, 0,0,0,  0,0,0,0,  0,0,0,0,  atlasW,0,0,0]
slot 14: [atlasW,0,0,0,  glyphX,0,0,0,  1,0,0,0,  1,0,0,0]
slot 15: [glyphX,0,0,0,  1,0,0,0,  1,0,0,0,  0,0,0,0]
slot 16: [0,0,0,0,  0,0,0,0,  atlasW,0,0,0,  glyphX,0,0,0]
slot 17: [0,0,0,0,  atlasW,0,0,0,  glyphX,0,0,0,  1,0,0,0]
slot 18: [1,0,0,0,  1,0,0,0,  0,0,0,0,  0,0,0,0]
slot 19: [1,0,0,0,  0,0,0,0,  0,0,0,0,  0,0,0,0]
```

The float values (e.g. `171.0`, `170.0`, `172.0`) correspond to **atlas pixel
dimensions and glyph offsets**. These matrices transform a unit quad into the
per-glyph UV sub-rectangle within the font atlas.

**Conclusion:** Ordinal 148 configures a **batched text render descriptor**. The
7 sub-pointers are per-glyph transformation matrices. The setup function at
`state_ptr[0]` (0x18023e24) applies these matrices via **texgen** to generate
per-vertex UVs from the unit position quad.

---

## 3. Draw Group Analysis

### Handle `0x100e38e0` — draws 9–14 (6 glyphs)

- **Position array** (`0x101aa218`): ONE quad, 10×12 pixels (single glyph cell)
- **UV array** (`0x101aa0a8`): ONE quad mapping to **full atlas** `(0.5,90.5)→(132.5,-0.5)`
  → `menuTetrisLogo_4444.pix` (132×91)
- **Per-draw translation** advances X by 10px each draw
- **Result with current fix**: renders the full atlas 6 times side by side.
  Visually incorrect — should render 6 different glyph sub-regions.

The guest intends each draw to sample a **different sub-rectangle** of the atlas,
selected by the texgen matrix from the ordinal-148 descriptor. Without texgen,
all 6 draws sample the same full-atlas region.

### Handle `0x100e5260` — draws 21–29 (9 glyphs)

- **Position array** (`0x101a3748`): ONE quad, 16×16 pixels (single glyph cell)
- **UV array**: **NONE DEFINED** — only array slot 0 (position) is set
- **Per-draw translation** advances X by 20px each draw
- **Result**: skipped (no UV data available)

This handle relies **entirely on texgen** from the state block to generate UVs.
There is no client UV array at all. The texture matrix in the state block
(offsets 0x1c–0x2c) defines the glyph-to-atlas mapping.

---

## 4. Why UV Span Is None

| Handle | Root Cause |
|--------|-----------|
| `0x100e38e0` | UV array exists but was rejected by epoch check (now bypassed for pointer handles); UVs decode to full-atlas mapping, not per-glyph sub-rects |
| `0x100e5260` | No UV array defined at all; UVs must come from texgen / texture matrix in state block |

The common missing piece: **texture coordinate generation (texgen)** using the
matrix parameters from the material state block and the ordinal-148 descriptor.

---

## 5. Correlation with Loaded Assets

The pointer-backed draws correlate with these loaded A8 font atlases:

| Atlas | Dimensions | Likely Use |
|-------|-----------|-----------|
| `menuTetrisLogo_4444.pix` | 132×91 | Handle 0x100e38e0 (full-atlas UV currently) |
| `f10x12text1/2/3_a8.pix` | 980×24 | 10×12 font (matches 0x100e38e0 glyph cell 10×12) |
| `f16x16menu1/2/3_a8.pix` | 1568×32 | 16×16 font (matches 0x100e5260 glyph cell 16×16) |
| `f13x13menu1/2/3_a8.pix` | 1276×26 | 13×13 menu font |
| `menus_a8.pix` | 320×99 | Menu UI elements |

The glyph cell dimensions (10×12 and 16×16) match the font atlas tile sizes
exactly, confirming these are **text rendering draws**.

---

## 6. Current Implementation Status

### Applied evidence-backed fixes

1. **Epoch bypass for pointer handles**: UV arrays from previous materials are
   reused regardless of epoch when the current handle is a work-RAM pointer.
   (Rationale: pointer-backed materials use shared client arrays, not their own
   epoch-tagged definitions.)

2. **Array slot 2 fallback**: When array slot 1 has 4 components (color data,
   not UVs), attempt to use array slot 2 for 2-component UVs.

3. **Bounded skip warnings**: First occurrence per (handle, reason) pair is
   logged; subsequent identical skips are suppressed to avoid log flooding.

### Resulting behavior

- `0x100e38e0` draws 9–14: **rasterize** (full atlas × 6 — visually wrong but
  no longer skipped)
- `0x100e5260` draws 21–29: **still skipped** (no UV source available without
  texgen)

---

## 7. Recovered Text/Glyph Runtime Path

Further tracing shows that both pointer-backed groups are **text builders**, not
logo-piece quads.

### 7.1 Draws 21–29: UTF-16 cursor on the caller stack

At the ordinal-137 callsite immediately before each draw, the caller stack
contains:

- `sp+0x08`: remaining-glyph counter (decrements 7,6,5,4,3,2,1,0)
- `sp+0x0c`: text object pointer (`0x101a3670`)
- `sp+0x10`: current UTF-16 cursor (`0x101a76c4`, `0x101a76c6`, ...)

The text buffer decodes to:

```
0x101a76c2: [0x0039, 0x0394, 0x0395, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0000]
            = "9ΔΕABCDE"
```

Per-draw advancement confirms this is a glyph loop:

| draw | cursor | current code unit |
|------|--------|-------------------|
| 21 | 0x101a76c4 | 0x0394 (Δ) |
| 22 | 0x101a76c6 | 0x0395 (Ε) |
| 23 | 0x101a76c8 | 0x0041 (A) |
| 24 | 0x101a76ca | 0x0042 (B) |
| 25 | 0x101a76cc | 0x0043 (C) |
| 26 | 0x101a76ce | 0x0044 (D) |
| 27 | 0x101a76d0 | 0x0045 (E) |
| 28 | 0x101a76d2 | 0x0000 (terminator after E) |

Draw 29 is a trailing flush / final emit after the glyph loop reaches the
terminator.

### 7.2 Draws 9–14: generated text from a scalar formatter

Disassembly around `0x18008480..0x1800857c` shows the guest is building a short
formatted string by repeatedly:

1. computing a character code (digits, `':'`, `'A'/'P'`, `'M'`),
2. calling `0x1801616c(text_obj, char)`,
3. then calling the draw helper through the import stub.

This group is also text, not a fixed atlas/logo strip.

### 7.3 Character lookup tables recovered from the font object

The text object's font pointer (`text_obj+0x14`) feeds two direct lookup tables:

- `font+0x0c` → `table_a[char]`
- `font+0x10` → `table_b[char]`

Recovered values for the draw-21 group:

| char | code | table_a | table_b |
|------|------|---------|---------|
| `9` | 0x0039 | 25 | 0 |
| `Δ` | 0x0394 | 0 | 5 |
| `Ε` | 0x0395 | 1 | 5 |
| `A` | 0x0041 | 33 | 0 |
| `B` | 0x0042 | 34 | 0 |
| `C` | 0x0043 | 35 | 0 |
| `D` | 0x0044 | 36 | 0 |
| `E` | 0x0045 | 37 | 0 |

This is strong evidence that:

- `table_a` is the **local glyph index / x-cell selector**
- `table_b` is a **segment/page selector**

### 7.4 Exact guest UV-generation helper recovered

The guest helper at `0x180161cc` builds the four generated UV vertices and writes
them into the state object via `0x1801d9e4`.

`0x1801d9e4` writes per-vertex coordinate pairs to:

- `state + 0x48 + vertex*8` → first coordinate
- `state + 0x4c + vertex*8` → second coordinate

for vertices 0..3.

The text helper path is:

1. `0x18007754(font, char)` → `table_a[char]`
2. `0x18007778(font, char)` → `table_b[char]`
3. `0x180074fc / 0x18007560 / 0x1800742c / 0x18007478` combine those table
   outputs with per-segment arrays at `font+0x60..0x74`
4. `0x180161cc` applies ±0.5 texel-center adjustments
5. `0x1801d9e4` stores the four UV corners into the active state object

### 7.5 Current status of the recovered texgen path

The per-segment arrays used by the recovered helper are still visible in the
font object dumps:

- `font+0x60`
- `font+0x64`
- `font+0x68`
- `font+0x6c`
- `font+0x74` (segment widths/counts)

Those arrays are still the right place to keep studying, but the renderer now has
a narrow evidence-backed fallback for both observed pointer-backed text groups:

- `draw21–29` rasterize using recovered text/font lookup data when the guest
  state UVs are degenerate.
- `draw9–14` also rasterize through the same generated-UV path once the text
  object is validated against the active state pointer and bad stack cursors are
  rejected.

### 7.6 Transform carry for glyph loops

A later headed/menu trace showed another text-specific accuracy issue: the first
pointer-backed glyph in a run had the correct base translation, while later
glyphs collapsed to `y=0` because the HLE reset translation after every draw.
The guest stream is more subtle:

- normal sprite draws issue a full translation before each draw, so the generic
  reset-after-draw behavior is fine;
- pointer-backed text runs issue a full base translation for the first glyph,
  then only per-glyph X deltas before subsequent `DrawArrays` calls;
- the next material bind starts a fresh text/run context.

The HLE now carries the accumulated translation only for work-RAM pointer-backed
text handles, and clears that carry on the next material bind/frame reset. This
keeps normal sprites isolated while allowing text glyphs to advance across the
line:

| group | before | after |
|---|---|---|
| `draw9–14` | first glyph near `(14,228)`, later glyphs near `(10,0)` | glyph bounds advance `(14,228)`, `(24,228)`, … `(64,228)` |
| `draw21–28` | first glyph near `(68.8,52)`, later glyphs near `(20,0)` | glyph bounds advance `(68.8,52)`, `(88.8,52)`, … `(208.8,52)` |

### 7.7 Directly isolated next missing sub-primitive

The next missing sub-primitive is now more precise than “generic texgen”:

> **Replace the bounded text cursor/object fallback with the exact guest
> formatter/texgen state transitions, especially around the scalar formatter path
> feeding `draw9–14` and the font segment metric arrays at `font+0x60..0x74`.**

The fallback is evidence-backed and bounded, but still not the final model. The
current `ordinal 148` no-op remains suspicious because its call record looks like
a generic “descriptor → target object/page” initializer. The direct evidence is
that draw-time text can now be recovered in the renderer, while the accurate
runtime-side initialization still needs to be modeled.

## 8. Next Missing Primitive

**Accurate guest texgen/formatter modeling, not hardcoded strings.**

Pointer-backed text now rasterizes, advances in screen space, and selects the
matching font atlas family (`f10x12*` / `f16x16*`) instead of unrelated small A8
UI strips. However, the visible menu text is still not content-accurate: current
headed evidence shows glyphs such as `9 ABCDE` where the real localized menu
strings should appear. The next step is to trace and model the guest
helper/state writes so both the cursor and generated UVs come from the same
state the game intended.

This should NOT be shortcut by hardcoding glyph rectangles or strings.

---

## 8. Audit: "patched 15 placeholder Tetris resource slots"

The existing patch in `maybe_patch_guest_state` (pc range 0x18013d4c–0x18014020)
allocates placeholder resource slots (entries + 0x200-byte payloads) for indices
20–37 in a resource array owned by guest register r9.

- **What it changes**: zeroes 18 array slots → fills slots 20–37 with allocated
  entry/payload pointers.
- **Why it exists**: the guest resource loader leaves these slots unpopulated
  (likely awaiting async file delivery); without the patch, the guest
  dereferences NULL and crashes.
- **Dependency**: the pointer-backed material objects (`0x100e38e0`,
  `0x100e5260`) are allocated **after** this patch runs and are independent of
  the placeholder slot contents. The patch does not populate the font atlas
  descriptors or texgen matrices.

This patch should NOT be expanded to inject texgen data. The correct path is to
implement texgen in the renderer.

---

## 9. Artifacts

- Pointer-handle object dumps: `/tmp/tetris_ptr_probe.log`
- Array content dumps: `/tmp/tetris_array_probe.log`
- Glyph table dumps: `/tmp/tetris_glyph_probe.log`
- Draw detail with decoded UVs: same logs, `draw_detail` lines
