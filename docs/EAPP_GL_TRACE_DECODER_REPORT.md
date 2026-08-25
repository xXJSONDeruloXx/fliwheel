# Tetris OpenGLES Trace Decoder Report

Fixture:
- `clicky-core/tests/fixtures/eapp/tetris_gl_trace.json`
- standalone renderer uses generated textures for replay tests

This report reflects the deeper capture pass that follows pointer-like stack words and records bounded snapshots, mapped regions, truncation status, and AsyncFileIO-backed file relationships.

---

## Frame signatures

Unique frames captured in `0..=50`:

| first..last | repeat | signature |
|---|---:|---|
| `0..0` | 1 | `649fcb2a79cab13c` |
| `1..1` | 1 | `c8b7f6e2987be4f5` |
| `2..2` | 1 | `c56e4b07b6eb9661` |
| `3..3` | 1 | `731b6601b1e54632` |
| `4..50` | 47 | `bd2ed153e8273927` |

Deduplication is **exact full-frame deduplication only**. Repeated calls inside a frame are preserved verbatim.

---

## Confirmed facts

### Ordinal 37
Confirmed by disassembly:
- `r0 = 7`
- `r1 = 0`
- `r2 = 4`
- call site sets these directly immediately before the import

So ordinal 37 is a confirmed `DrawArrays(7, 0, 4)` boundary.

### Ordinal 99 argument layout
Confirmed by disassembly at the upload site:

```text
r0 = target
r1 = level
r2 = internal_format
r3 = width
sp+0x00 = height
sp+0x04 = border
sp+0x08 = format
sp+0x0c = pixel_type
sp+0x10 = source_ptr
```

This is direct call-site evidence, not just pattern matching.

### Ordinal 137/40/159/175 direct arguments vs stale stack locals
From disassembly:
- `137`: direct args are `r0..r3` plus stack args at `sp+0`, `sp+4`
- `40`: direct arg is only `r0`; `r1..r3` and stack are caller leftovers
- `159`: direct args are only `r0`, `r1`; `r2..r3` are caller leftovers
- `175`: direct args are `r0`, `r1`, `r2`; `r3=0`
- `37`: direct args are `r0`, `r1`, `r2`; `r3` and stack are caller leftovers

---

## One fully decoded texture upload candidate

Chosen candidate: the `50 × 50` RGBA5551 upload used by the standalone renderer test.

Triplet in frame 2:
- seq 12: ordinal 45
- seq 13: ordinal 4
- seq 14: ordinal 99

Decoded upload:

```text
Ordinal45Prep
  r0 = 1
  r1 = 0x1802d5b4   descriptor_ptr
  r2 = 50           prep_width
  r3 = 50           prep_height
  ret = 0

Ordinal4State
  r0 = 0x0de1       target = GL_TEXTURE_2D
  r1 = 0
  r2 = 50
  r3 = 50
  ret = 0

Ordinal99Upload
  r0      = 0x0de1  target = GL_TEXTURE_2D
  r1      = 0       level = 0
  r2      = 0x1908  internal_format = GL_RGBA
  r3      = 50      width
  sp+0x00 = 50      height
  sp+0x04 = 0       border
  sp+0x08 = 0x1908  format = GL_RGBA
  sp+0x0c = 0x8034  pixel_type = GL_UNSIGNED_SHORT_5_5_5_1
  sp+0x10 = 0x100145d6 source_ptr
  ret     = 0
```

Source-pointer relationship to AsyncFileIO:

```text
source_ptr  = 0x100145d6
file        = eaLogo_5551.pix
buffer_base = 0x10014590
offset      = 70
file_len    = 5072
mapped_region = work_ram
truncated   = false for the bounded source snapshot
```

This is the clearest fully decoded upload candidate in the trace.

---

## One fully decoded four-vertex position array

Chosen candidate: frame 4, quad 3, seq 32, ordinal 137, stack pointer at `sp+0x04`.

```text
pointer = 0x101b7068
components = 4
format = 0x140c (GL_FIXED)
```

Snapshot decoded as signed 16.16 fixed-point, grouped as XYZW per vertex:

```text
v0 = (  0.0,   0.0, 0.0, 1.0)
v1 = (  0.0,  50.0, 0.0, 1.0)
v2 = ( 50.0,  50.0, 0.0, 1.0)
v3 = ( 50.0,   0.0, 0.0, 1.0)
```

This is a clean four-vertex local-space rectangle.

---

## One fully decoded four-vertex UV array

Chosen candidate: frame 4, quad 3, seq 35, ordinal 137, stack pointer at `sp+0x04`.

```text
pointer = 0x101b70a8
components = 2
format = 0x140c (GL_FIXED)
```

Decoded as signed 16.16 fixed-point pairs:

```text
uv0 = ( 0.5, 49.5)
uv1 = ( 0.5, -0.5)
uv2 = (50.5, -0.5)
uv3 = (50.5, 49.5)
```

This is consistent with texel-centered nearest-neighbor UVs for a `50 × 50` texture.

---

## Translation values and texture identifier

For the same quad, the preceding state calls are:

```text
seq 29: ordinal 169   r1 = 260.0   r2 = 129.0
seq 30: ordinal 169   r1 = -25.0   r2 = -50.0
seq 31: ordinal 159   r0 = 0x1b    texture/state identifier
```

Conservative decoded translation used by the standalone test:

```text
translation = (260.0 + -25.0, 129.0 + -50.0)
            = (235.0, 79.0)
```

Applied to the local position array, this yields final screen-space corners:

```text
(235,  79)
(235, 129)
(285, 129)
(285,  79)
```

The `0x1b` texture/state identifier is real fixture data. Its exact mapping back to the upload sequence remains an interpretation, but the dimensions strongly match the `50 × 50` upload above.

---

## Exact sequence connecting this quad to the confirmed draw

Frame 4, quad 3:

```text
29  Ordinal169State   r1=260.0   r2=129.0
30  Ordinal169State   r1=-25.0   r2=-50.0
31  Ordinal159State   r0=0x1b    r1=0x101b6fc0
32  Ordinal137Array   r0=0 r1=4 r2=0x140c sp+4=0x101b7068   position XYZW
33  Ordinal40State    r0=0
34  Ordinal40State    r0=1
35  Ordinal137Array   r0=1 r1=2 r2=0x140c sp+4=0x101b70a8   UV ST
36  Ordinal175State   r0=0x18025508 r1=0x18025488 r2=0x180254c8
37  Ordinal125State   r0=0 r1=1 r2=0 r3=0x18025508
38  DrawArrays        r0=7 r1=0 r2=4
39  Ordinal36State    r0=1
```

This is the smallest verified dataflow from decoded arrays to the confirmed draw boundary.

---

## Ordinal meanings: status labels

### Confirmed by disassembly or direct argument evidence
- `37` = `DrawArrays(7, 0, 4)`
- `99` = upload-like call with the exact argument layout documented above
- `137` = array-format call with direct stack arguments at `sp+0`, `sp+4`
- `40` = state call with only `r0` directly set at the call site
- `159` = state call with only `r0`, `r1` directly set at the call site
- `175` = state call with direct `r0`, `r1`, `r2`

### High-confidence interpretation
- `45` = upload preparation / descriptor setup
- `4` = target/state setup preceding upload
- the `50 × 50` upload candidate above is the likely backing texture for quad 3
- the summed translation `(235, 79)` is the correct placement for quad 3

### Unresolved
- exact semantic names for `45`, `4`, `159`, `169`, `175`, `125`, `36`
- whether `0x1b` is a texture handle, descriptor index, or another state identifier
- exact semantic role of the third/other array slots in quads 1 and 4
- whether 158/157 should be called “present” / “submit” independent of ordering evidence

---

## Conservative semantic model

Keep only the confirmed draw name. Use neutral names elsewhere:

```text
Ordinal169State(...)
Ordinal159State(...)
Ordinal137Array(index, components, format, stack_args...)
Ordinal40State(index)
Ordinal175State(...)
Ordinal125State(...)
DrawArrays(7, 0, 4)
Ordinal36State(...)
```

---

## Frame 4 replay summary

The standalone replay now renders the complete steady-state frame-4 stream using generated textures only.

### Artifact comparison

| artifact | hash | nonzero | bbox | alpha range | draw4 effect |
|---|---|---:|---|---|---|
| draws 1–3 only | `0xb1b233a9858cfcc3` | 76800 | `0,0–319,239` | `255..255` | baseline |
| all draws with draw4 disabled | `0xb1b233a9858cfcc3` | 76800 | `0,0–319,239` | `255..255` | same as baseline |
| all draws (current unresolved draw4 probe) | `0x3514598dae7f1fe2` | 76800 | `0,0–319,239` | `255..255` | full-screen overlay changes 76796 pixels vs baseline |
| draw4 only (A8 placeholder / alpha probe) | `0x05c580c350a40325` | 76800 | `0,0–319,239` | `128..128` | blends |
| draw4 only (opaque probe) | `0x24cda718d8961325` | 76800 | `0,0–319,239` | `255..255` | overwrites |

### Per-draw summary

| draw | seq | handle | translation | bounds | proposed texture | confidence | coverage |
|---|---:|---:|---|---|---|---:|---:|
| 1 | `15` | `19` | `(0.0, 0.0)` | `(0,0)–(320,240)` | `screenBG_565.pix` / `320×240` / `RGB565` | 0.93 | 76800 |
| 2 | `27` | `14` | `(42.5, 76.0)` | `(42.5,76.0)–(277.5,238.0)` | `tetrisLogo_4444.pix` / `250×162` / `RGBA4444` | 0.84 | 38070 |
| 3 | `38` | `27` | `(235.0, 79.0)` | `(235,79)–(285,129)` | `eaLogo_5551.pix` / `50×50` / `RGBA5551` | 0.87 | 2500 |
| 4 | `48` | `3` | `(0.0, 0.0)` | `(0,0)–(320,240)` | unresolved full-screen overlay/material blob | 0.28 | 76800 |

### Relevant state grouped with each draw

- **Draw 1**: `169×3` → `159` → `137`/`40`/`137`/`40`/`137` → `175` → `125` → `37` → `36` → `36`
  - aux `137` seq `11` is present and still unresolved as a secondary 4-component array.
- **Draw 2**: `169×2` → `159` → `137` → `40` → `40` → `137` → `175` → `125` → `37` → `36`
- **Draw 3**: `169×2` → `159` → `137` → `40` → `40` → `137` → `175` → `125` → `37` → `36`
- **Draw 4**: `169` → `159` → `137` → `40` → `137` → `40` → `175` → `125` → `37` → `36`
  - the second `137` seq `44` decodes as an all-ones 4-component array, which fits a white/tint/identity-style overlay more than an ordinary textured quad.

### Dataflow notes

- `Ordinal159State` is best read as a small-handle selector for a texture/material composite; `r1` carries the per-draw state blob.
- The three obvious frame-4 handles (`19`, `14`, `27`) line up with the earlier upload triplets by size and file-backed payload.
- Handle `3` does **not** line up with a captured upload triplet; its state blob looks generated and full-screen.
- I still did not capture the exact table write / later load that stores the small handle into the `Ordinal159` call path.

### Replay semantics recovered so far

- Texture rows are consumed in file order; the replay sampler uses floor+clamp nearest-neighbor sampling.
- UVs in the trace are half-texel centered (`±0.5`) and do not need an extra correction in replay.
- The current quad split is seam-free with the rasterizer's winding-normalized triangle rule.
- `A8`, `RGB565`, `RGBA5551`, and `RGBA4444` are all supported in the standalone renderer.
- Alpha-bearing textures use source-over compositing.

### Conservative mapping table

The standalone local replay currently maps these assets by captured dimensions/format rather than a proven handle→asset write path; the handle linkage remains inferred from the trace.

| upload triplet | source file | descriptor/object ptr | candidate handle | frame-4 draw | confidence | missing evidence |
|---|---|---:|---:|---|---:|---|
| `9→10→11` | `screenBG_565.pix` | `0x1802d57c` | `19` | draw 1 | 0.93 | exact table write not captured; matched by size + fullscreen state blob |
| `12→13→14` | `tetrisLogoT_4444.pix` | `0x1802d5b4` | `14` | draw 2 | 0.84 | exact table write not captured; matched by size + state blob |
| `15→16→17` | `eaLogo_5551.pix` | `0x1802d73c` | `27` | draw 3 | 0.87 | exact table write not captured; matched by size + state blob |
| none captured | none captured | `0x101b7260` | `3` | draw 4 | 0.28 | no matching upload triplet; appears to be a generated full-screen overlay/material blob |

### Deterministic replay artifacts

```text
draws_1_3_hash = 0xb1b233a9858cfcc3
all_draws_hash = 0x3514598dae7f1fe2
draw4_alpha_hash = 0x05c580c350a40325
draw4_opaque_hash = 0x24cda718d8961325
```

Optional inspection artifact:
- set `CLICKY_WRITE_TETRIS_FRAME4_PPM=1`
- run `cargo test -p clicky-core --test eapp_gl_decode replay_frame4_produces_complete_artifact_and_hash -- --nocapture`
- it writes `/tmp/tetris_frame4_replay.ppm`

### Orientation investigation (local real assets)

The real-asset replay was tested against the extracted `Games_RO/66666` .pix files with separate texture and screen-space variants. The current trace geometry reads as:

- draw 2 / handle 14: translation `(42.5, 76.0)`, current bounds `(42.5,76.0)–(277.5,238.0)`
- draw 3 / handle 27: translation `(235.0, 79.0)`, current bounds `(235.0,79.0)–(285.0,129.0)`

Mirroring the framebuffer/presentation origin moves those to:

- draw 2: `(42.5,2.0)–(277.5,164.0)`
- draw 3: `(235.0,111.0)–(285.0,161.0)`

That mirrored layout matches the splash composition better than the unflipped output. The per-vertex and rectangle-aware Y transforms reach the same mirrored bounds, but they remain geometry questions, not texture-row questions. I found no explicit viewport/projection state in the offline replay path, so the coordinate-system evidence still stops at screen-space positioning.

```text
current                         = 0xaae9cac7ffba5133
screen_origin_best              = 0xce8b5d920ca880cb
no_texture_framebuffer_vflip     = 0xce8b5d920ca880cb
per_vertex_screen_y_flip         = 0xc3c9d9654c08b738
rectangle_aware_screen_y_flip    = 0xf02275af7682e89d
texture_vflip_no_framebuffer     = 0x868dea7c858c11b2
texture_vflip_framebuffer_vflip  = 0x9fc1e1d7e21b8982
uv_vflip_framebuffer_vflip       = 0x13fdb28d17a78bf2
hflip_control                    = 0xac56d21fe8cbd5bd
both_axis_control                = 0xebcfdc66fa6b385d
```

Selected texture convention: **current row order preserved** (do not bake row inversion into local asset loading yet).

Selected screen-space convention: **framebuffer presentation flip / PPM-output flip** for the offline replay diagnostics.

### Live OpenGLES HLE (experimental, Gate A confirmed)

A first minimal live HLE render pass is available behind explicit flags. It
reuses the standalone rasterizer, texture decoders, framebuffer hashing, and
PPM writer (no second live-only renderer) and is staged in two gates.

Flags:
- `CLICKY_EXPERIMENTAL_GL_HLE=1` — enable the live path. When unset, the
  legacy fill-color diagnostic path is used unchanged.
- `CLICKY_GL_PRESENT_VFLIP=0|1` — configurable vertical presentation flip
  (default 1). Applied only when serializing/presenting, never baked into
  texture decode, UV decode, vertex transforms, or rasterizer storage.
- `CLICKY_GL_GATE_B=1` — copy the presented framebuffer to the eapp window.
  Off by default; intended to be enabled only after Gate A is coherent.
- `CLICKY_GL_LIVE_CONTINUOUS=1` — capture every 4-draw frame instead of the
  first steady-state frame.

Scope preserved:
- texture row order preserved;
- captured UVs and guest geometry preserved;
- internal 320×240 rasterizer framebuffer kept in native (unflipped) order;
- vertical flip is a presentation/serialization convenience only, not a
  confirmed ABI rule.

Gate A output (Tetris, live guest, default mode):
```text
frame=4 draws=4
internal_hash  = 0xdc2cbe3857ad3483   # == offline real_draws_1_3
presented_hash = 0x55462dde9fead727     # internal + present_vflip
pixel_diff_vs_offline(internal vs draws_1_3) = 0 / 76800  (EXACT MATCH)
```

Live draw summary (matches offline frame-4 analysis):

| live draw | handle | inferred dim | bounds | coverage | status |
|---|---|---|---|---:|---|
| 1 | `0x13`(19) | 320×240 RGB565 | (0,0)-(320,240) | 76800 | rasterized |
| 2 | `0xe`(14) | 250×162 RGBA4444 | (42,76)-(278,238) | 38070 | rasterized |
| 3 | `0x1b`(27) | 50×50 RGBA5551 | (235,79)-(285,129) | 2500 | rasterized |
| 4 | `0x3` | 1×1 | (0,0)-(320,240) | 0 | skipped (unresolved overlay) |

Artifacts:
- `/tmp/tetris_live_gl_hle_internal.ppm` (native order)
- `/tmp/tetris_live_gl_hle_presented.ppm` (with optional vflip)

Live-vs-offline conclusion: the live composition **structurally matches** the
offline replay and is **pixel-exact** for the three resolved draws (background,
Tetris logo, EA logo). The only delta is the unresolved handle-3 full-screen
overlay (draw 4), which is intentionally skipped live and remains unresolved in
both the live and offline analysis. It therefore also matches
`/tmp/tetris_frame4_orientation_screen_origin_best.ppm` structurally (same four
draws, same bounds/formats/handles), differing only by that overlay tint.

Gate B (`CLICKY_GL_GATE_B=1`) copies the presented buffer to the eapp render
state; confirmed firing under its flag. Window presentation orientation is left
configurable via `CLICKY_GL_PRESENT_VFLIP` pending visual confirmation.

Continuous rendering lifecycle evidence (live, first steady frames):
```text
158(h0x3f001),165(h0x180254c8),...37#1...37#2...37#3...37#4...,157(h0x0)
```

Observed ordering supports neutral lifecycle labels:
- `158`: candidate frame begin (always first surface ordinal, before all steady-state draws)
- `165`: surface/setup state (immediately after 158, before draws)
- `157`: candidate frame present/end (always last surface ordinal, after all steady-state draws)

Continuous mode (`CLICKY_GL_LIVE_CONTINUOUS=1`) now avoids the one-shot repeated-signature heuristic. It assembles frames from the observed 158→157 lifecycle, keeps an active internal buffer separate from completed/presented buffers, and publishes only completed non-empty frames. Gate B copies the completed presented buffer under the render-state mutex, so the eapp window does not read a partially rendered frame. The earlier four-draw-only presentation gate was a temporary splash-validation guard; once guest time advanced, it hid legitimate 3-draw loading/splash frames and later 29-draw menu/loading frames by pinning the window to the last 4-draw splash frame.

Headless continuous validation with Gate B enabled:
```text
frame_diag idx=2 begin=158@208 end=157@257 draws=4 sig=[0x13,0xe,0x1b,0x3]
  internal=0xdc2cbe3857ad3483 presented=0x55462dde9fead727 skipped=1 status=presented
frame_diag idx=3.. stable: same hashes/signature repeated
```

Detected transitional anomalies are bounded and reported:
- startup `candidate_present without active frame` before steady rendering;
- frame 2 had draws outside active 158→157 and a 10-draw transitional candidate, discarded;
- steady frames repeat identically; first stable presented hash remains `0x55462dde9fead727`.

Runtime progression root cause (Tetris splash hang): `miscTBD:9` is confirmed, for Tetris, to write an advancing monotonic time value through the pointer in `r0`. A universal semantic name for the ordinal is still unproven, so keep the label conservative: "pointer-writing monotonic-time API" rather than a global ABI name. The aux frame callback calls it with `r0=app_object+4` and `r0=app_object+8`, then computes `delta = [app+8] - [app+4]`. With the old handler (`return args[0]`, no write), `[app+4]`, `[app+8]`, and the splash timeout timestamps stayed zero indefinitely while guest frames and async callbacks continued. Guest constants at `0x180256c0/0x180256c4` are `4_000_000` and `2_000_000`; observed behavior matches microsecond units.

`miscTBD:9` now writes host monotonic microseconds through `r0`. This is a runtime-behavior recovery, not an arbitrary delay or state-forcing shortcut. With `CLICKY_STARTUP_PROGRESS_TRACE=1`, bounded diagnostics include module/ordinal, args, return value, host monotonic time, pointed value before/after, and whether guest time advanced. The same trace records per-frame framebuffer hash, candidate time values, pending async request count, callback count, and first hash-change frame.

Post-fix evidence with live rendering enabled:
```text
startup_progress frame=120 app_time_current≈3_900_000 app_time_delta≈13_000 pending=0 callbacks=51
startup_progress frame=193 fb_hash changed to 0xe48b120366ee7539 app_time_current≈5_600_000
```

Async startup I/O evidence: 51 `AsyncFileIO:3` requests were resolved, 51 callbacks were queued, 51 callbacks were dispatched, and `pending_async=0` before the visual transition. The common callback at `0x1801fc68` consumes request fields `[0x08]`, `[0x20]`, and `[0x24]`; the observed callback path does not depend on the `0xffffffff` diagnostic fields at `[0x28..0x30]`.

After time starts advancing, Tetris legitimately emits non-4-draw completed frames (3-draw splash/loading phase, then 29-draw menu/loading frames). Continuous Gate B now publishes any completed non-empty 158→157 frame; rasterization behavior is unchanged. This prevents the window from staying pinned to the last 4-draw splash frame after the guest has progressed.

External behavioral reference (direct Nano 3G hardware observation, not an emulator-forcing rule):
1. initial splash appears;
2. legal/EULA-like text is visible during startup;
3. a loading bar progressively fills left-to-right;
4. the main menu then slides into place;
5. in the main-menu composition, the Tetris logo is at the upper-left/top area;
6. the current EA-logo placement and some other emulator-rendered elements are not yet confirmed accurate.

Startup visual-accuracy capture/classification after the time fix and live-renderer extensions (`CLICKY_STARTUP_CAPTURE_DIR=/tmp/tetris_startup_capture_verified`, `CLICKY_GL_LIVE_CONTINUOUS=1`, `CLICKY_GL_GATE_B=1`, `CLICKY_GL_PRESENT_VFLIP=1`):

```text
guest frame 2:     10 draws, transitional double-composition, presented hash 0x46bffc2ca3d18ba0
guest frames 3-35: 4 draws [0x13,0xe,0x1b,0x3], per-frame fade/light overlay hashes
guest frames 36-192: 3 draws [0x13,0xe,0x1b], stable splash/loading hold, hash 0x55462dde9fead727
guest frame 193+: 29 draws, menu/loading composition, hash 0x014afe9414f068b1 in the bounded capture
```

Evidence-backed live-renderer fixes from that capture:

- `GL_RGBA` + `GL_UNSIGNED_BYTE` (`0x1908/0x1401`) is supported as `Rgba8888`, which resolves the `battery_8888.pix` upload instead of logging it unsupported.
- UV sub-rects now match the smallest decoded upload that contains the UV extents after exact-dimension matching fails. This renders atlas-backed A8 strips such as `menus_a8.pix` instead of skipping them solely because the draw covers a sub-region like `308x34` inside a `320x99` upload.
- Array definitions carry a material epoch. UV/color decoding only uses arrays from the current ordinal-159 material epoch, preventing stale slot-1 arrays from being interpreted as texcoords or colors after a later bind only redefines array 0.
- Handle `0x3` 4-component `GL_FIXED` slot-1 arrays are rendered as conservative solid-color quads. This recovers the observed startup fade/light overlay and several solid menu-transition fills without treating unrelated pointer-like handles as solid colors.

Optional dumping: `CLICKY_GL_DUMP_FRAMES=N` writes the first N completed presented frames to `/tmp/tetris_live_frame_XXXX.ppm`.

Handle→upload association is still *inferred* by live upload dimensions/format
(logged as inferred), not a proven handle-creation path; the exact ordinal-159
handle-to-upload mapping remains open.

### Unresolved-state list

- exact descriptor/object → handle write/load path for `Ordinal45`/`Ordinal4`/`Ordinal99` → `Ordinal159`
- draw 1 secondary `137` seq `11` role
- draw 4 secondary `137` seq `44` role beyond the identity/tint interpretation
- pointer-like handles `0x100e38e0` and `0x100e5260`: after epoch filtering they no longer reuse stale UV/color arrays, but their array-0-only/menu text or glyph path is still unresolved
- legal/EULA-like text and full menu text are still not represented in rendered pixels; do not synthesize them without resolving the live ABI evidence
- whether the generated replay textures correspond to the real game art (they do not attempt to)
