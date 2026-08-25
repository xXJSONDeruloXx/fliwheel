# Vortex (12345) Fix Gameplan

## Problem Statement

Vortex crashes with `FatalMemException` at `pc=0x18014d58`, `fault_addr=0x4` (null pointer write). This is one of 3 crashers (iQuiz, Vortex, Texas Hold'em) blocking 19% of the clickwheel game library.

## Root Cause Analysis

### Fault Details
- **PC**: `0x18014d58` (file offset `0x14d38`)
- **Fault**: `kind=Write fault_addr=0x00000004`
- **Code**: Structure-fill routine at `0x14d44-0x14d54`:
  ```armasm
  0x14d44: ldr r0,[r0,#4]    ; read buffer pointer from object+4
  0x14d54: stmia r0!,{r7,r9} ; write to buffer
  0x14d58: stmia r0!,{r4,r5,r6,lr} ; <-- FAULT: r0 was null+8
  ```
- **Register state**: `r0=0x8` (destination advanced from null by one `stmia` pair), `lr=0x00010000` (invalid return)

### Call Chain
1. Function at `0x14d38` is called from `0x7868: bl 0x206b0`
2. Caller at `0x7854-0x7868`:
   ```armasm
   0x7854: str r0, [r4]         ; writes container vtable
   0x7858: ldr r1, [r4, #0x54]  ; reads count from container+0x54
   0x7864: ldr r0, [r4, #0x5c]  ; reads array ptr from container+0x5c
   0x7868: bl 0x206b0           ; calls release loop
   ```
3. `[object+4]` (the buffer pointer) is null because an upstream HLE call failed to populate it

### Suspected Missing HLE: OpenGLES:165

From the investigation notes:
- `OpenGLES:165` is currently a no-op stub
- It is described as "surface handle" / "beginFrame / bindContext"
- The call signature: `r0=ctx, r1=ptr, r2=vtx_buf_ptr`
- Vortex likely expects this to write a framebuffer/buffer pointer into a state object

## Hypothesis

Vortex calls `OpenGLES:165` early in GL setup, expecting it to:
1. Allocate or bind a surface/framebuffer
2. Write the buffer pointer into a state object at a known offset
3. This pointer is later read at `[object+4]` and used as a destination for structure fills

Since `OpenGLES:165` is currently a no-op (returns 0 without writing anything), the pointer remains null, causing the crash when the game tries to write to it.

## Fix Strategy

### Option A: Minimal Surface Bind (Recommended)

Implement a minimal `OpenGLES:165` that:
1. Recognizes when it's being called with a surface/context bind request
2. Allocates or returns a synthetic framebuffer pointer (work-RAM backed)
3. Writes the pointer into the expected state object location

Implementation approach:
```rust
// In OpenGLES:165 handler
if r1 != 0 {
    // r1 points to a state object where we should write the buffer ptr
    // Allocate or reuse a synthetic surface buffer in work-RAM
    let surface_buf = allocate_surface_buffer(width, height);
    // Write the pointer into the state object at the expected offset
    write_guest_u32(r1 + 4, surface_buf)?;
}
```

### Option B: Passthrough to Live GL

If `OpenGLES:165` maps to a standard GL ES 1.1 call (like `eglMakeCurrent` or surface bind), we could:
1. Map the guest context to a host GL context
2. Have the host GL allocate the surface
3. Return a handle that satisfies the game's expectations

This is more complex and may not be necessary for Vortex to boot.

### Option C: Detect and Skip the Write

Instead of fixing the null pointer, detect when the game is about to write to null and:
1. Log the skip
2. Return early from the structure-fill function
3. Hope the game continues (may cause visual corruption but could allow boot)

This is a hack and not recommended.

## Validation Plan

### Phase 1: Confirm the Hypothesis
1. Add `CLICKY_EAPP_TRACE=OpenGLES:165` logging to capture all calls
2. Run Vortex headed for ~1 second to see if `OpenGLES:165` fires before the crash
3. Check register args to confirm the expected pattern (`r0=ctx, r1=ptr, r2=vtx_buf_ptr`)

### Phase 2: Implement Minimal Fix
1. Implement Option A (minimal surface bind)
2. Run Vortex headed smoke test (8 seconds)
3. Verify: no crash, draws rendered > 0

### Phase 3: Regression Testing
1. Run all 16 games smoke test
2. Verify the 13 working games still work (0 fatals, draws stable)
3. Check Texas Hold'em (33333) - may share the same fix
4. Check iQuiz (11002) - different crash site, may need separate fix

### Phase 4: Verify Visual Output (if Vortex boots)
1. Run Vortex headed for longer duration (30 seconds)
2. Check if frames are visually coherent (not just noise)
3. Compare to expected Vortex gameplay screenshots if available

## Risk Mitigation

### Risk: Breaking Other Games
Mitigation:
- Gate the `OpenGLES:165` fix behind a check for Vortex-specific patterns (e.g., specific `r1` value range, or only enable after detecting Vortex binary)
- Or, implement as a generic but minimal fix that only writes when `r1` points to valid work-RAM and current value is null
- Run full regression before committing

### Risk: Fixing Vortex But Not Texas Hold'em
Mitigation:
- Texas Hold'em likely shares the same root cause (same engine family)
- After Vortex fix, immediately test Texas Hold'em
- If different crash, investigate separately

### Risk: iQuiz is Different
Mitigation:
- iQuiz crashes at `pc=0x18001b08` (memcpy null destination)
- This is a different code pattern (memcpy vs structure-fill)
- May be `Metadata` object provider gap, not GL surface
- Accept that iQuiz may need separate investigation

## Success Criteria

| Criterion | Target |
|-----------|--------|
| Vortex boots without crash | ✅ 0 fatals in 8s headed run |
| Vortex renders draws | ✅ > 100 draws rendered |
| No regressions in other games | ✅ 13/13 working games still work |
| Texas Hold'em also fixed | ✅ or ❌ (document if separate) |

## Timeline

1. **Phase 1** (15 min): Add tracing, confirm hypothesis
2. **Phase 2** (30 min): Implement minimal `OpenGLES:165` fix
3. **Phase 3** (20 min): Run regression test
4. **Phase 4** (15 min): Visual verification if successful

Total: ~80 minutes of focused work

## Related Issues

- iQuiz (11002): `pc=0x18001b08` - memcpy null destination, likely different root cause
- Texas Hold'em (33333): Same crash family as Vortex, test after fix
- `OpenGLES:165` may also help LOST (1B200) which has zero draws after texture fix

## Code Pointers

- Current `OpenGLES:165` handler: `clicky-core/src/sys/eapp/mod.rs`
- Search for `OpenGLES::Ordinal(165)` or similar pattern
- The ordinal handlers are typically in a large match statement
- Vortex binary: `Games_RO/12345/Executables/vortex_1_1_2563290.bin`
- Load address: `0x18000000` (same as all games)

## Notes from Documentation

From `IPOD_GAMES_BRINGUP.md`:
> - `12345` Vortex: `FatalMemException pc=0x18014d58 kind=Write off=0x00000004`
>   - null-destination struct-fill at `pc=0x18014d58` after inline GL uploads
>   - `[object+4]` buffer ptr is null, likely GL surface bind gap

From investigation section:
> - Vortex: likely GL surface bind (`OpenGLES:165` "surface handle")

The documentation confirms our hypothesis. The fix is to implement `OpenGLES:165` surface binding.

## Iteration 28 Update — r10 literal-pool clobber + chained null-object faults

The original minimal `OpenGLES:165`/container-write hypothesis was incomplete.
Vortex does call `OpenGLES:165` early with `r0=0x18063ebc`, but the later crash
helper is not simply using `[container+4]` as the final destination.

### New finding: `0x18014d38` clobbers `r0` from a literal-pool register block

Crash helper sequence:

```armasm
0x18014d38: push ...
0x18014d44: ldr   r0, [r0, #4]          ; initial container/object deref
0x18014d48: ldmia r10!, {r0,r1,r2,r3,r7,r8,r9,r10}
0x18014d54: stmia r0!, {...}
0x18014d58: stmia r0!, {...}            ; original fatal, r0=0x00000008
```

`0x18014d48` overwrites `r0` after the container deref, loading it from a
literal-pool-backed register block. In the failing run this makes `r0=0x8`, so
writing `container+4` is insufficient even when the work-RAM structures are
preallocated correctly.

### Work attempted this iteration

Implemented a tightly gated Vortex-only compatibility shim:

- bootstrap preallocates a work-RAM `work_container`, `surface_buf`, and object;
- stores those addresses at `WORK_RAM_BASE+0xff0/0xff4/0xff8`;
- exact-PC hooks redirect near-null block-copy destinations to the scratch
  surface/object only for bundle path containing `12345`:
  - `0x18014d54` original struct-fill crash site;
  - `0x18011290` sibling struct-fill crash site;
  - `0x18018ae8`/`0x18018aec` null object write path;
  - `0x18013e00`/`0x18013e04`/`0x18013e08` null object read path.

### Result

The shim is not a final fix, but it proves progress:

1. Vortex passes the original `pc=0x18014d58` fatal.
2. It passes the sibling `pc=0x18011294` fatal.
3. It reaches `OpenGLES:37` draw submission for the first time in this workstream.
4. It then exposes a later fault:

```text
pc=0x1800ab14 fault_addr=0x00000024 kind=Write
r0=0x00010000 r4=0x180bdd90 r5=0x180654b4 r6=0x18063e6c
```

Current log: `/tmp/vortex_iter28_hook7.log`.

### Updated assessment

Vortex is not just missing one `OpenGLES:165` surface pointer. It has a chain of
binary-local register-block/null-object assumptions around the early GL setup.
Best next fix should avoid unbounded exact-PC hooks if possible:

- locate and emulate the register-block/literal-pool structure producer that
  should populate the values consumed by `0x18014d38` and `0x18011274`; or
- implement a more principled Vortex surface/context object model that makes
  those register blocks point at work-RAM destinations; or
- if continuing with compatibility shims, keep them bundle+PC gated and stop
  after each newly exposed fault to avoid regressing the 13 working titles.

### Other crashers captured with current build

- iQuiz (`11002`): `OpenGLES:165` occurs, then fatal
  `pc=0x18001b08 fault_addr=0x0000000c kind=Write`, `r4=0`.
  Log: `/tmp/11002_iter28_crash.log`. This looks like a separate null-object
  write after Audio/Metadata setup, not the same Vortex r10 register-block crash.
- Texas Hold'em (`33333`): reaches frame 1, `OpenGLES:37`, `OpenGLES:157`, and
  AsyncFileIO callbacks, then fatal
  `pc=0x1802fd00 fault_addr=0x00000008 kind=Write`, `r0=0` inside the
  AsyncFileIO callback chain. Log: `/tmp/33333_iter28_crash.log`. This appears
  closer to an async-completion/owner-callback ABI gap than Vortex's surface
  block-copy issue.

## Iteration 29 Update — Vortex boots past fatal via mutable state-block wiring

Iteration 28's exact-PC surface/object redirects exposed another repeated Vortex
pattern: multiple initializer helpers load a global object, then write a series
of GL/fixed-point fields through `[global+4]`. Those `[+4]` fields are null in
our runtime, so the writes fault at small addresses.

### New decoded sites

`0x1800aa40..0x1800ac48` initializes a ~0xa0-byte mutable state block through
`[r4+4]`:

```armasm
0x1800ab08: ldr r1, [r4, #4]
0x1800ab10: str r0, [r1, #0x24]   ; previous fatal exposed in iter 28/29
0x1800ab1c: str r0, [r1, #0x4c]
0x1800ab28: str r0, [r1, #0x74]
0x1800ab30: str r0, [r1, #0x9c]
```

`0x18013eec..0x18013f1c` does the same through literal global
`0x180bdda8`:

```armasm
0x18013ef4: ldr r1, [0x180bdda8 + 4]
0x18013efc: str r0, [r1, #0x10]
0x18013f00: str r0, [r1, #0x0c]
0x18013f14: str r2, [r1, #0x60]
```

### Fix attempted

The Vortex bootstrap preallocation now also allocates a 0x200-byte work-RAM
`state_block` and stores it at `WORK_RAM_BASE+0xffc`. The Vortex-only exact-PC
shim wires this block into the two decoded `[global+4]` state-block slots and,
where needed, repairs the live destination register for the current write.

This is still bundle-gated (`12345`) and PC-range-gated; it does not affect
Tetris or other games.

### Result

Vortex now survives the smoke window that previously crashed repeatedly:

```text
log: /tmp/vortex_iter29_stateblock2.log
fatal=0
OpenGLES:37=1
OpenGLES:157=1
frame returns logged through frame 18000 during the timeout window
```

Regression checks:

- `cargo test -p clicky-core --lib eapp` -> 16 passed
- Tetris headed smoke `/tmp/tetris_iter29_regression.log` -> `fatal=0`,
  `skipped_nonzero=0`

### Remaining caveat

The implementation is still a compatibility shim rather than a fully decoded
Vortex GL object model. It is much less arbitrary than the first surface-buffer
redirect because the new block matches the decoded initializer writes, but the
long-term cleanup should replace the PC ranges with a principled model of the
Vortex mutable GL state objects if more titles share this pattern.
