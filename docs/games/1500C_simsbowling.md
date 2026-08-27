# Sims Bowling (Bundle 1500C)

**Status:** 🟡 TITLE SCREEN + PARTIAL FOLLOW-UP | **Evidence:** shared NDC projection and rectangle-target paletted upload now rasterize the title and a small `The` follow-up text element; gameplay/menu transition is not yet reached | **Engine:** Sims Engine

## Quick Start
```bash
./scripts/games/simsbowling.sh
./scripts/games/simsbowling.sh --timeout 15
./scripts/games/simsbowling.sh --headless
```

## Bundle Info
- **Executable:** `SimsBowling_1_1_3002478.bin` (eapp format)
- **Asset Format:** `.wav` (31 files) + `.rlb` resource bundle

## Assets
- **Audio:** `.wav` and `.m4a` files (a-g musical notes + sfx)
- **Resources:** `gameLib.rlb` (game library bundle)

## Notable
- Sims engine variant with different asset loading
- Uses `.rlb` resource library format
- Lower draw count — simpler UI than other games
- The title screen is spatially coherent after fixing the lowercase runtime
  bundle ID (`1500c`) in the shared NDC projection matcher. The live GL HLE
  also accepts Bowling's `GL_TEXTURE_RECTANGLE` paletted upload, so the
  one-draw follow-up now includes a small legible `The` text element. The menu
  and game scene are still not reached.

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

The optional Sims async flags stage the 19,997,809-byte Bowling RLB and let the
guest parse several real resource entries. The probe still ends in a black
progress state without a menu or gameplay scene, so it is retained as
reverse-engineering evidence rather than a default fix. See the [RLB stream
probe](../game_tests/20260827_sims_rlb_stream_probe.md).

The first observed Bowling follow-up reads are 4,096 bytes at offset `0`, then
63,527 bytes at offset `0x1000`, followed by additional guest-derived entries.
The `payload` result alias keeps this experiment independent of the allocator's
current synthetic address.
