# Lost (1B200) — Comprehensive Analysis & Experiment Log

**Status:** ❌ NO GFX | **Draws:** 0 | **Engine:** Lost Engine (rserver.bin render server)  
**Last updated:** 2026-06-26

---

## 1. Architecture

Lost is architecturally unique among iPod click wheel games. It uses a dedicated **GL Render Server** binary (`rserver.bin`) that contains the iPod's OpenGL ES driver implementation.

```
┌──────────────────────────────────────────────────────────────┐
│  iPod GL Architecture (Real Device)                          │
│                                                              │
│  ┌──────────────┐   IPC   ┌────────────────────────────────┐ │
│  │  Game Binary  │───────▶│  rserver.bin (Render Server)  │ │
│  │  (0x18000000+)│        │  - GL driver (gldMalloc, etc) │ │
│  │  Game logic,   │        │  - ShaderMachine (USSE code)  │ │
│  │  scene graph,  │        │  - FrontBufferA compositor    │ │
│  │  input handling│        │  - Display control (TV/MPU)   │ │
│  └──────────────┘          └────────────────────────────────┘ │
│          │                            │                       │
│          │ GL ordinal imports          │ Direct HW access      │
│          ▼                            ▼                       │
│    ┌───────────────┐         ┌──────────────────┐            │
│    │ Import Thunks  │         │ iPod LCD Driver   │            │
│    │ (0x1F000000+)  │         │ (PowerVR MBX)     │            │
│    └───────────────┘         └──────────────────┘            │
└──────────────────────────────────────────────────────────────┘
```

### rserver.bin

- **Size:** 105,020 bytes (0x19A3C)
- **Loaded to:** Guest address 0x10001038 (via AsyncFileIO:3)
- **Structure:**

| Offset | Size | Content |
|--------|------|---------|
| 0x000 | 0x200 | Dispatch table (all zeros, filled by ordinal 164 on real HW) |
| 0x200 | ~0x10C00 | USSE shader microcode + data tables |
| 0x10C24 | ~0x200 | `RenderServerVersion:RELEASE:2704` and config strings |
| 0x10C60+ | ~0x300 | Display/power config strings: "power", "freeze", "model", "default", "dac", "svideo", "mode", "width", "height", "wide" |
| 0x10DFC+ | ~0x200 | `FrontBufferA`, `display control`, `display map`, `TV buffer`, `MPU stripe`, `Image stripe` |
| 0x11000+ | varies | Zero-initialized data regions (populated at runtime) |

### Key Embedded Strings

| String | Meaning |
|--------|---------|
| `RenderServerVersion:RELEASE:2704` | GL driver version |
| `gldCalloc` / `gldMallocSlow` / `gldVecMalloc` / `gldVecCalloc` | GL driver memory allocation |
| `ShaderMachine: Attempt to set uniform when no shader is bound` | Shader state machine |
| `ShaderMachine: Invalid shader type found` | Shader type validation |
| `Length is less than data described in texture sub data header` | Texture upload validation |
| `FrontBufferA` | iPod front buffer identifier |
| `display control` / `display map` | Low-level display API |
| `TV buffer` / `MPU stripe` / `Image stripe` | Hardware display layers |
| `getpower` / `powersave` / `encoding` / `backlight` | Power management |
| `%d=%d` | Likely uniform name/value pairs |

### USSE Microcode (offset 0x200+)

The code section is **NOT ARM or Thumb instructions**. It's PowerVR MBX USSE
(Universal Scalable Shader Engine) microcode — a GPU-specific instruction format.
Disassembly as ARM or Thumb produces invalid/nonsensical instructions.

---

## 2. Init Sequence

### Non-GL imports (before frame loop)

| Import | Args | Notes |
|--------|------|-------|
| miscTBD:0 | r0=0x7FF80 | malloc(524160) — large allocation, likely render server heap |
| miscTBD:9 | r0=stack, r1=0x10502B00, r2=0x90000, r3=0x78000 | Time/tick API |
| AsyncFileIO:3 | path=rserver.bin | Load 105,020 bytes to 0x10001038 |
| miscTBD:6 | r0=2, r1=0x10012038, r2=0x180401FF, r3=0x180401D1 | **Render server comm?** r1 is inside rserver region |
| Settings:0 | key=Language | Language query |
| Audio:52 | r0=0x10001038 (rserver base!) | Audio passes rserver address — coordination |
| Audio:51 | r0=0, r1=0x55 | Audio init |
| Audio:56 | r0=0, r1=0x1000B038 | Audio |

### GL imports (init frame)

```
153 → viewport(0, 0xFFFF, 0xFFFF, 0)   // Set viewport
164 → shader_create(r0=1, r1=rserver_addr, r2=0xFFFFFFFF)
152 → program_query(r1=buf_ptr, r2=size_ptr)
153 → viewport(0, 0, 0, 0)              // Reset viewport
152 → program_query(r1=buf_ptr, r2=size_ptr)
4   → bind_texture(GL_TEXTURE_2D, 0)    // Bind for upload
99  → upload_2d(GL_TEXTURE_2D, 0, LA8, 122×10)   // Text label atlas 1
4   → bind_texture(GL_TEXTURE_2D, 0)    // Re-bind
99  → upload_2d(GL_TEXTURE_2D, 0, LA8, 42×10)    // Text label atlas 2
```

### Frame loop (repeats forever)

```
13  → glCullFace(0)            // No culling
12  → glClear(0x4000)          // Clear color buffer  
159 → bind_material(0xe, 0x18060910)  // Shader material
157 → present(0x0)             // Show frame (nothing drawn)
```

**0 draws per frame. No vertex setup, no draw calls.**

---

## 3. Material Object

The shader material at `state_ptr=0x18060910` when `handle=0xE`:

```
Offset  Value           Interpretation
0x00    0x00000000      Flags / status
0x04    0x00000000      Flags / status
0x08    0x00000988      Texture 1 data size (2440 = 122×10×2 LA8 bytes)
0x0C    0x0000007A      Texture 1 width (122)
0x10    0x0000000A      Texture 1 height (10)
0x14    0x0000007A      Texture 1 width (confirm)
0x18    0x0000000A      Texture 1 height (confirm)
0x1C    0x00000000      Padding
0x20    0x0000190A      Texture 1 format: GL_LUMINANCE_ALPHA (0x190A)
0x24    0x00001401      Texture 1 type: GL_UNSIGNED_BYTE (0x1401)
0x28    0x00000000      Padding
0x2C    0x00000001      Texture 1 index / count
0x30    0x00000001      Texture 1 index
0x34    0x00000348      Texture 2 data size (840 = 42×10×2 LA8 bytes)
0x38    0x0000002A      Texture 2 width (42)
0x3C    0x0000000A      Texture 2 height (10)
0x40    0x0000002A      Texture 2 width (confirm)
0x44    0x0000000A      Texture 2 height (confirm)
0x48    0x00000000      Padding
0x4C    0x0000190A      Texture 2 format: GL_LUMINANCE_ALPHA
0x50    0x00001401      Texture 2 type: GL_UNSIGNED_BYTE
0x54    0x00000000      Padding
0x58    0x00000002      Texture 2 index / count
0x5C    0x00000002      Texture 2 index
0x60    0xFFFFFFFF      **Shader state (-1 = not compiled)**
0x64..  0x00000000×7    All zeros
```

---

## 4. Experiments & Results

### Experiment 1: Baseline
**Setup:** Default export handling (ordinal 164 returns 1, 152 returns success)  
**Result:** 0 draws per frame  
**Frame loop:** `13, 12, 159, 157`

### Experiment 2: miscTBD:6 Return Value
**Setup:** `CLICKY_MISCTBD6_RET=1` (returns 1 instead of 0)  
**Rationale:** miscTBD:6 is called with r1 pointing inside rserver region — might be a render server communication channel whose return value gates drawing  
**Result:** 0 draws per frame  
**Frame loop:** `13, 12, 159, 157` (unchanged)

### Experiment 3: miscTBD:6 Large Return Value
**Setup:** `CLICKY_MISCTBD6_RET=9999`  
**Rationale:** Return value might be a handle or capability ID  
**Result:** 0 draws per frame (unchanged)

### Experiment 4: Rserver Header Fill (incrementing values)
**Setup:** `CLICKY_EAPP_FILL_RSERVER_HEADER=1` — fills 128 header words with values 1..128  
**Rationale:** The 0x200-byte header at 0x10001038 is all zeros; filling it would satisfy any null-pointer checks  
**Result:** 0 draws per frame (unchanged)  
**Conclusion:** The header values are NOT the sole gating factor

### Experiment 5: Rserver Header Fill + miscTBD:6
**Setup:** Both CLICKY_EAPP_FILL_RSERVER_HEADER=1 and CLICKY_MISCTBD6_RET=1  
**Result:** 0 draws per frame (unchanged)

### Experiment 6: Thumb Stub Function Pointers
**Setup:** `CLICKY_EAPP_THUMB_STUBS=1` — allocates two ARM Thumb stubs in work RAM:
- `stub0`: `mov r0, #0; bx lr` (returns 0)
- `stub1`: `mov r0, #1; bx lr` (returns 1)

Fills all 128 header entries with `stub1 | 1` (Thumb mode bit set).  
**Rationale:** On real HW, ordinal 164 writes function pointers into the header. If the game reads these as function pointers and calls them, valid Thumb stubs returning 1 would indicate "success"  
**Result:** 0 draws per frame (unchanged)  
**Conclusion:** The game DOESN'T call the header function pointers before deciding not to draw. The draw-blocking condition is checked BEFORE the stubs would be called.

### Experiment 7: Shader State Patch
**Setup:** Write 0 to [state_ptr+0x60] (was 0xFFFFFFFF) during present handler  
**Rationale:** 0xFFFFFFFF at offset 0x60 looks like a "not compiled" flag  
**Result:** Patch applies once (0xFFFFFFFF → 0), but game still 0 draws in subsequent frames  
**Conclusion:** The shader state flag is necessary but not sufficient

### Experiment 8: Skip rserver.bin Load
**Setup:** `CLICKY_EAPP_SKIP_RSERVER=1` — don't load rserver.bin over game binary  
**Rationale:** If the game's original code (before overwrite) has a rendering path that doesn't need rserver  
**Result:** 0 draws per frame (the original game binary also has zeros at 0x10001038)  
**Conclusion:** The game was DESIGNED to have rserver.bin overwrite its early code

### Experiment 10: Mass -1 Patching
**Setup:** `CLICKY_EAPP_LOST_PATCH_NEG1=1` — patches all `0xFFFFFFFF` values in 4 heap regions to 0 each frame

Results:
- Frame 0: 150 patches (initial clear)
- Frame 2: 98 patches (game re-writes -1s each frame)
- Frame 3+: 2 patches (steady state)
- **Still 0 draws per frame**

**Conclusion:** The 0xFFFFFFFF markers are not the sole blocking condition. The game re-creates them each frame, and even zeroing them out before the next frame's decision doesn't enable drawing. The blocking condition is elsewhere in the game code's execution path — possibly a variable that's never set by the render server's initialization, or a conditional branch that depends on the render server's USSE code actually executing and producing output.

### Experiment 11: Splash Screen Injection
**Setup:** `CLICKY_EAPP_LOST_SPLASH=1` — Loads `lostLaunch.raw.lcd5` (320×216 RGB565, 16-byte header) from the game bundle and writes it into the DMA framebuffer on frame 0

Results:
- Splash image data successfully written to DMA framebuffer
- DMA overlay system picks up the data and composites it
- **Static splash screen visible in headed mode**
- No interactive game rendering (expected — this is a static fallback)

**Conclusion:** The DMA overlay system works for Lost. The splash screen shows the game's title art. However, this is purely cosmetic — the game's GL rendering pipeline still produces 0 draws. Interactive gameplay would require the render server to actually function.

### Experiment 12: Audio:52 Divide-by-Zero Fix
**Setup:** Audio:52 returns 1 when called with r0>=0x10000000 (rserver base). Audio:51 returns 1 when called with r3>=0x10000000 (shared data pointer). Ordinal 153 viewport fixup applies 320×240 default when w=0 or h=0.

Results:
- **Divide-by-zero eliminated** — no more "Arithmetic exception: Divide By Zero"
- Game now progresses past init — tries to load `options.sav`
- Game heap shows fewer 0xFFFFFFFF markers
- **Still 0 draws per frame**

**Conclusion:** The divide-by-zero was caused by Audio:52 returning 0 when called with the rserver base address. The game's ARM code at 0x180054EC divides Audio:51's return by Audio:52's return. Both returning 0 gave 0/0 → crash. With Audio:52=1 and Audio:51=1, the division gives -1/1 = -1, which is valid and the game continues. However, the rendering is still gated by the rserver dispatch table not being populated.

### Experiment 13: Render Function Analysis
**Method:** ARM disassembly of the game binary's render code path

Key discoveries:
1. **Render function at 0x18007264** — contains `PUSH {r0-r11, lr}`, calls ordinal 19, and full rendering logic. But it's NEVER called from the main frame loop!
2. **Main loop at 0x1803B924** — calls only `ordinal 13` (via `B 0x18000098`), then ordinal 12, then ordinal 159 (bind material), then ordinal 157 (present)
3. **The render function is called from scene management code** at `0x18028DA8`, `0x18039E8C`, `0x1803A360` — NOT from the frame loop
4. **Ordinal 19** (at import stub `0x180000B0`) is the key render dispatch call — it's only called from inside the render function, which is never reached
5. The game's architecture: main loop does display management, scene system does rendering. The scene system is not activated because the rserver init didn't create any active render contexts.

**Conclusion:** The root cause is architectural — the game separates "display management" (main loop) from "scene rendering" (render function). The scene system needs the rserver to create render contexts. Without a working rserver, no scenes are active, so the render function is never called, so ordinal 19 is never called, so no draws happen.

### Experiment 14: Rserver Data Structure Analysis
**Method:** Full memory dump of rserver data region (0x10012038) at frame 10

Key findings in the 59 non-zero words:
- `+0x0048=0x00066000` — buffer/memory size
- `+0x0070=0x1001F520` — pointer into rserver ROM area
- `+0x0164=0x10080F88` — pointer into work RAM (allocated buffer)
- `+0x02D8=0x656e5553` = "USne" (locale string for US English)
- **`+0x0734=0xCAFEBABE`** — Java-style magic structure marker
- `+0x0738=0x5C` — structure length = 92 bytes
- `+0x0750=0x00130010` — PowerVR shader format descriptor
- `+0x076C=0x00010018` — shader program metadata
- **Three CAFEBABE-tagged blocks** contain shader/render state descriptors
- Next-level pointers: `0x100127E4`, `0x10012824`, `0x10012864` (all within rserver ROM)
- `+0x0708=0x1001F4B4` — pointer to GL context state

**Conclusion:** The rserver data region IS partially initialized by the game (59 words). The CAFEBABE markers indicate structured shader/render state objects. But the critical dispatch table (function pointers for rendering) is missing — it would be populated by the iPod's GL driver during shader compilation (ordinal 164).

### Experiment 15: USSE Parser Scaffold
**Implementation:** Added `clicky-core/src/sys/eapp/usse.rs` and wired OpenGLES:164 to parse/cache the loaded `rserver.bin`.

Runtime result:

```text
ordinal_164: parsed_usse base=0x10001038 bytes=105020 code=0x200..0x10c24 words=17033 strings=80 version=RenderServerVersion:RELEASE:2704 first=[+0x0200=0x0c602f78, +0x0204=0x2f790001, +0x0208=0x00025a3f, +0x020c=0x2eb96fc1, +0x0210=0x10c11c39, +0x0214=0x2f630075]
```

What it does now:
- Locates apparent USSE/code region at offset `0x200`
- Stops code region at `RenderServerVersion:RELEASE:2704` string offset (`0x10c24`)
- Caches 17,033 32-bit words for future opcode semantics
- Extracts embedded printable strings
- Provides `UsseVm` placeholder state and ordinal-19 execution hook

Current limitation:
- This is a parser/VM scaffold, not full PowerVR MBX USSE semantics yet. It creates the concrete execution object needed for incremental opcode implementation.

### Experiment 17: Main Loop Render-Call Patch
**Implementation:** Added env-gated `CLICKY_EAPP_LOST_PATCH_RENDER_CALL=1`.

Patch:
- At load time, replace instruction at `0x1803B924`
- Original: `0xEBFF2E4D` (`BL 0x18007260`, trivial wrapper to OpenGLES:13)
- Patched: `0xEBFF2E4E` (`BL 0x18007264`, full render submission helper that calls OpenGLES:19)

Result:

```text
lost_patch_render_call: patched 0x1803b924 BL target 0x18007260 -> 0x18007264
OpenGLES:19 pc=0x1f000260 lr=0x180072f8 r0=0x00000000 r1=0x00000000 r2=0x00000000 r3=0x3f800000
ordinal_19: render_dispatch ... usse_pc=64 executed=64 ...
```

Conclusion:
- This confirms the hidden render submission path can be reached from the main loop.
- The game now repeatedly calls OpenGLES:19 under the patch.
- Still 0 rasterized draws because ordinal 19 currently only advances the placeholder USSE VM.
- Next target: implement ordinal 19 as the render-server draw/dispatch operation, using the CAFEBABE descriptor blocks and the parsed USSE program state.

### Experiment 16: Runtime Rserver Block Scanner
**Implementation:** Added `UsseProgram::scan_runtime_blocks()` and wired it into `miscTBD:6 cmd=2`, where Lost passes the initialized rserver data pointer (`0x10012038`).

Observed runtime blocks:

```text
rserver_blocks:
  addr=0x1001276c off=0x0734 len_words=92 preview=[0xcafebabe,0x0000005c,0x00000000,0x00000000,0x00000140,0x000000f0,0x08080008,0x00130010]
  addr=0x100127c8 off=0x0790 len_words=20 preview=[0xcafebabe,0x00000014,0x100127e4,0x10012824,0x10012864,0xcafebabe,0x00000040,0x00000000]
  addr=0x100127dc off=0x07a4 len_words=64 preview=[0xcafebabe,0x00000040,0x00000000,0x00000000,...]
  addr=0x1001281c off=0x07e4 len_words=64 preview=[0xcafebabe,0x00000040,0x00000000,0x00000000,...]
  addr=0x1001285c off=0x0824 len_words=64 preview=[0xcafebabe,0x00000040,0x00000000,0x00000000,...]
  addr=0x1001289c off=0x0864 len_words=51208 preview=[0xcafebabe,0x0000c808,0x00000000,0x00000000,...]
```

Important interpretation:
- Block 0 contains screen dimensions `0x140` × `0x0f0` (320×240)
- Block 0 also contains descriptor words `0x08080008` and `0x00130010`
- Block 1 appears to be a pointer table to three subsequent blocks
- The very large final block (`0xC808`) is likely a command/data buffer, not a word count in the same sense as the smaller descriptor blocks

---

## 5. Splash Screen Data

File: `lostLaunch.raw.lcd5` (138,256 bytes)  
Format: 16-byte header + 320×216 RGB565 pixel data

| Offset | Value | Meaning |
|--------|-------|---------|
| 0x00 | 320 | Width |
| 0x04 | 216 | Height |
| 0x08 | 640 | Stride? |
| 0x0C | '565L' | RGB565 format marker |
| 0x10+ | 138,240 bytes | 320×216 RGB565 pixels |

This is the splash/title screen image available for fallback rendering.

---

## 6. Root Cause Analysis

Lost's 0-draw behavior is caused by the **game code's decision logic** that runs before calling any GL draw ordinals. This decision code is in the game binary at `0x18000000+` and executes within the CPU emulator.

The decision tree approximately looks like:

```
Game Frame Loop:
  1. Read render server state from shared memory
  2. Check if render server is initialized and operational
  3. If NOT operational: skip all drawing, go to present
  4. If operational: set up vertex arrays, issue draw calls
```

**The problem is step 2-3.** Even though we've tried:
- Valid program handles (ordinal 164)
- Successful program queries (ordinal 152)
- Non-zero dispatch table (header fill)
- Valid function pointers (Thumb stubs)
- Successful miscTBD:6 returns
- Clear shader state flags

...the game STILL decides the render server is not operational.

**Most likely cause:** The game reads from a **shared memory region** that the render server writes to during its initialization. On a real iPod, after rserver.bin is loaded and ordinal 164 is called, the render server process would:
1. Parse the USSE microcode
2. Set up internal state (shader compilation tables, buffer allocations)
3. Write status pointers/flags to a **different memory region** (not the rserver header)
4. The game reads this status region to decide if drawing is safe

This shared memory region is likely the large allocation from miscTBD:0 (r0=0x7FF80 = 524,160 bytes). The game allocates this BEFORE loading rserver.bin, passes a pointer to it via rserver header or miscTBD:6, and expects the render server to write a "ready" flag somewhere in this buffer.

---

## 7. Paths Forward (Priority Order)

### A. Memory Region Identification (Most Promising)
Scan the large allocation buffer (0x10502B00, 524KB) for writes after rserver.bin is loaded. The render server code might write status values here during init. Look for non-zero values appearing in this region after ordinal 164 returns.

**Implementation:** Periodic scan of the 524KB buffer for changes, or read-watchpoints.

### B. USSE Microcode Interpretation
Build a USSE shader interpreter that can:
1. Parse the microcode at rserver.bin offset 0x200+
2. Execute shader programs for each frame
3. Write results to the framebuffer

This is the most complete solution but requires reverse engineering the PowerVR MBX USSE instruction format, which is poorly documented.

### C. ARM Binary Patching
Find and patch the game's conditional branch that gates the draw calls. This requires:
1. Proper ARM disassembly of the game binary (respecting ARM/Thumb switching)
2. Identifying the branch that tests render server readiness
3. NOPing the branch or forcing it to always take the "ready" path

### D. Fallback Splash Screen Rendering
Inject the `lostLaunch.raw.lcd5` splash image into the DMA framebuffer as a cosmetic workaround. This wouldn't produce interactive gameplay but would show the game's title screen.

### E. Full Render Server Emulation
Implement ordinal 164 to actually parse rserver.bin and set up the complete rendering pipeline. This includes:
- USSE microcode parser
- Shader compilation pipeline
- Render server state machine
- FrontBufferA compositor

This is essentially reimplementing Apple's GL driver for the iPod — a massive engineering effort.

---

## 8. Env Var Reference

| Variable | Purpose | Default |
|----------|---------|---------|
| `CLICKY_MISCTBD6_RET` | Return value for miscTBD:6 | 0 |
| `CLICKY_EAPP_FILL_RSERVER_HEADER` | Fill header with incrementing values | disabled |
| `CLICKY_EAPP_THUMB_STUBS` | Fill header with Thumb stub pointers | disabled |
| `CLICKY_EAPP_LOST_SPLASH` | Inject lostLaunch splash into DMA framebuffer | disabled |
| `CLICKY_EAPP_LOST_MEMSCAN` | Scan memory regions at frame 10 | disabled |
| `CLICKY_EAPP_LOST_PATCH_NEG1` | Patch all 0xFFFFFFFF values to 0 each frame | disabled |
| `CLICKY_EAPP_SKIP_RSERVER` | Skip loading rserver.bin | disabled |

---

## 9. Game Data Files

The Lost game bundle contains extensive assets:

| Type | Files | Notes |
|------|-------|-------|
| Splash screen | `lostLaunch.raw.lcd5` | 320×216 RGB565, 16-byte header |
| Episode data | `d1`-`d15` | Episode-specific scene data (185KB-433KB) |
| Location data | `l`, `l1`-`l26` | Location images (101KB-1.5MB each) |
| Sound banks | `soundbank_*.dat` | 9 sound banks (165KB-2MB) |
| Audio | `0.mp3`-`15.mp3/m4a` | Background music and sound effects |
| Resources | `resources/{en,de,es,fr,it,ja}/` | Localized text resources |
