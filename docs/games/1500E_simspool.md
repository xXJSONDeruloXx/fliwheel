# Sims Pool (Bundle 1500E)

**Status:** 🟡 TITLE SCREEN + PARTIAL FOLLOW-UP | **Evidence:** shared NDC projection rasterizes the title and the ordinary atlas path now shows a small `The` follow-up text element; gameplay/menu transition is not yet reached | **Engine:** Sims Engine

## Quick Start
```bash
./scripts/games/simspool.sh
./scripts/games/simspool.sh --timeout 15
./scripts/games/simspool.sh --headless
```

## Bundle Info
- **Executable:** `SimsPool_1_1_3023310.bin` (eapp format)
- **Asset Format:** `.wav` (30 files) + `.rlb` resource bundle

## Assets
- **Audio:** `.wav` and `.m4a` files (similar to Sims Bowling)
- **Resources:** `gameLib.rlb` (game library bundle)

## Notable
- Sister game to Sims Bowling, shares gameLib.rlb
- Similar draw count and asset structure
- Sims engine variant
- The title screen is spatially coherent after fixing the lowercase runtime
  bundle ID (`1500e`) in the shared NDC projection matcher. The follow-up
  one-draw state now shows a small colored `The` text element through Pool's
  ordinary `297x75` atlas upload, but remains incomplete.

See the [2026-08-27 NDC casing-fix probe](../game_tests/20260827_sims_ndc_casing_fix.md)
and [rectangle-target texture probe](../game_tests/20260827_sims_paletted_texture_target.md)
for the exact commands, coverage, hashes, and captures.

## Environment
```bash
FLIWHEEL_EXPERIMENTAL_GL_HLE=1
FLIWHEEL_GL_GATE_B=1
FLIWHEEL_GL_LIVE_CONTINUOUS=1
FLIWHEEL_GL_PRESENT_VFLIP=1
```

The optional Sims async flags stage the 19,440,133-byte Pool RLB and let the
guest parse several real resource entries. The probe still ends without a menu
or gameplay scene, so it is retained as reverse-engineering evidence rather
than a default fix. See the [RLB stream probe](../game_tests/20260827_sims_rlb_stream_probe.md).

Pool's first observed follow-up reads are 4,096 bytes at offset `0`, then
55,607 bytes at offset `0x1000`, followed by additional guest-derived entries.
The `payload` result alias keeps this experiment independent of the allocator's
current synthetic address.

The optional input-ready experiment now reaches Pool's stable guest state 6
after the RLB reads, but that path still issues no menu or gameplay draw. The
title-specific address and receipt are documented in the [focused state-6
probe](../game_tests/20260827_sims_pool_state6_boundary.md); the input write
remains diagnostic-only and is not enabled by default.
