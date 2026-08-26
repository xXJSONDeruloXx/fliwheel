# eApp init-vector lifecycle probe

Date: 2026-08-26 UTC  
Repository: `fliwheel`

## Finding

The decrypted eApp header contains a vector table at `+0x14`. The first
nonzero vector is the entry routine, the next nonzero vector is a one-time
initialization routine, and the last nonzero vector is the recurring frame
callback. The reference loader in
`/Users/danhimebauch/Developer/ipod-emulator/tools/eapp-loader/src/bin/play.rs`
drives those vectors in order with two valid scratch context pointers before
entering its frame loop.

fliwheel now has the same lifecycle path available with:

```text
FLIWHEEL_EAPP_INIT_VECTORS=1
```

When enabled, the HLE supplies two zeroed 0x400-byte work-RAM contexts to the
init vector and resumes the normal app/frame setup after it returns. The
default path remains unchanged while the init-time contracts are incomplete.

## Evidence

The opt-in path completes without a fatal for `1B200` LOST:

```text
FLIWHEEL_EAPP_INIT_VECTORS=1 CYCLES=30000000 \
RUN_ROOT=/tmp/fliwheel_initoptin_1B200_20260826 \
./scripts/test_decrypted_games_interactive.sh \
  '/tmp/clicky_hle_eval.1i3DER/archive20/20 iPod games/Games_RO' 1B200
```

Result: exit 0, 700 guest frames, no fatal signatures. LOST still produces no
GL draws because its render-server shader path is unresolved.

Forcing the vector by default exposed missing shared contracts in the current
HLE: `14004` faults in an init-time texture/object path, and `1500C`/`1500E`
fault in the first render-server frame after init. Those failures are retained
as negative evidence in `/tmp/fliwheel_initvector_b_20260826/` and are why the
flag is opt-in rather than part of the default corpus matrix.

The default-path regression for those affected titles remains clean:

```text
/tmp/fliwheel_initgated_14004_20260826/interactive_matrix.md
/tmp/fliwheel_initgated_1500C_20260826/interactive_matrix.md
/tmp/fliwheel_initgated_1500E_20260826/interactive_matrix.md
```

## Next step

Trace the init-time allocations and the Sims render-server handoff against the
firmware-backed loader, then promote the flag only after every decrypted title
has a no-fatal vector regression and the affected titles gain useful content.
