# Texas Hold'em (Bundle 33333)

**Status:** ✅ WORKS | **Draws:** 34,088 (10s) | **Engine:** Hold'em Runtime

## Quick Start
```bash
./scripts/holdem.sh
./scripts/holdem.sh --timeout 15
./scripts/holdem.sh --headless
```

## Bundle Info
- **Executable:** `HoldEm_1_1_2563291.bin` (eapp format)
- **Splash:** `Holdem.raw.lcd5` (RGB565)
- **Asset Format:** `.ipd` (111 files) + `.blob` (15 files)

## Assets
- **Textures:** `.ipd` files loaded via `AsyncFileIO:3`
- **Background music:** `c.m4a`, `t.m4a`
- **Characters:** `Characters/` directory
- **Locations:** `Locations/` directory
- **Fonts:** `Fonts/Euro/ArialBold15.ipd` (A8 alpha font atlas)
- **Resources:** `Data/textures.txt`, localization in `Resources/`

## Notable
- Uses `Filesytem` import module (but doesn't depend on it for init)
- Loads `.ipd` font atlases successfully through AsyncFileIO:3
- Second-highest draw count — full poker table rendering

## Environment
```bash
CLICKY_EXPERIMENTAL_GL_HLE=1
CLICKY_GL_GATE_B=1
CLICKY_GL_LIVE_CONTINUOUS=1
CLICKY_GL_PRESENT_VFLIP=1
```
