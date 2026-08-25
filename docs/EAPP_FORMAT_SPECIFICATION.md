# EAPP Format Specification

## Overview

EAPP ("eapp" = executable application) is the executable format used by iPod Click Wheel games. Files are encrypted with FairPlay DRM when distributed but decrypted to this format during iPod sync.

## File Structure

```
+0x0000: eapp Header (0x28 bytes minimum)
+0x0028: ARM Code and Data
+...   : Sections, tables, strings
```

## Header Format (0x28 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0x00 | 4 | `magic` | Magic number: `"eapp"` (0x65617070) |
| 0x04 | 4 | `load_addr` | Load address: `0x10001000` (constant) |
| 0x08 | 4 | `version` | Format version: `5` |
| 0x0c | 4 | `hdr_size` | Header size: `0x28` (40 bytes) |
| 0x10 | 4 | `code_start` | ARM address where code begins (e.g., `0x1800002c`) |
| 0x14 | 4 | `data_end` | End of code/data (e.g., `0x1802094c`) |
| 0x18 | 4 | `bss_start` | Start of BSS segment (e.g., `0x18020948`) |
| 0x1c | 4 | `reserved` | Always `0x00000000` |

## Memory Layout

```
0x10001000: EAPP header loaded here (40 bytes)
0x10001028: First instruction or data
    ...
0x1800002c: Code entry point (field_10 from header)
    ...
0x18020948: BSS segment starts (field_18 from header)
0x1802094c: End of data (field_14 from header)
    ...
0x1...: Heap/stack (grows upward)
```

## Version 5 Format Details

All observed games use version 5 of the format.

### Load Address
The constant `0x10001000` suggests:
- iPod 5G/5.5G use a specific memory map
- Kernel reserves low 256MB (0x00000000-0x0FFFFFFF)
- User space starts at 0x10000000
- EAPP offset by 0x1000 for header

### Code Entry
The `code_start` field (0x1800002c in Bejeweled) is the ARM entry point. This is:
- Where execution begins after loading
- An absolute address, not a file offset

## ARM Code Analysis

First instructions at offset 0x28 in file (0x1800002c in memory):

```
0x00000000  nop              ; Alignment padding
0x180209a8  ...              ; Data reference
0xeafffffe  bal 0x18000030   ; Branch to entry
```

The `bal` (branch always with link) at offset 0x30 appears to jump to the actual initialization code.

## Sections After Header

### ARM Code
- Starts at file offset 0x28
- Loaded to `code_start` address
- Thumb-2 or ARMv5 instructions

### Data Tables
- String references
- Function pointer tables
- Game assets (small assets, not textures/audio)

### BSS Segment
- Uninitialized data
- Zeroed at load time
- Size = `data_end - bss_start`

## Game Assets

Large assets (textures, audio, levels) are stored separately:

| Pattern | Format | Purpose |
|---------|--------|---------|
| `tex_*.bin` | Raw/PVR | Textures |
| `snd_*.bin` | PCM/PAML | Audio |
| `lvl_*.bin` | Custom | Level data |
| `*.mod` | MOD/IMPM | Music modules |

Asset files are also FairPlay-encrypted with per-file keys.

## Observed Values Across Games

| Game | load_addr | version | code_start | data_end | bss_start |
|------|-----------|---------|------------|----------|-----------|
| Bejeweled | 0x10001000 | 5 | 0x1800002c | 0x1802094c | 0x18020948 |
| Cubis2 | 0x10001000 | 5 | 0x1800002c | 0x180384d0 | 0x180384cc |
| HoldEm | 0x10001000 | 5 | 0x1800002c | 0x180315c8 | 0x180315c4 |

Common patterns:
- All use same `load_addr` (0x10001000)
- All use same `version` (5)
- All use same `code_start` (0x1800002c)
- Different sizes for code/data based on game complexity

## Loader Requirements

An emulator/loader for EAPP files needs to:

1. **Parse header** (40 bytes at offset 0)
2. **Map memory** at `load_addr` (0x10001000)
3. **Copy code** from file offset 0x28 to `code_start`
4. **Zero BSS** from `bss_start` to `data_end`
5. **Set PC** to `code_start` and begin execution
6. **Provide syscall interface** for iPod OS functions

## Relationship to Encrypted Files

```
Encrypted (DRM)          Decrypted (EAPP)
+------------------+     +------------------+
| FairPlay Header  |     | eapp Header      |
| (0x400+ bytes)   | --> | (0x28 bytes)     |
+------------------+     +------------------+
| Encrypted Data   |     | ARM Code + Data  |
| (AES-128-CTR)    | --> | (Plain)          |
+------------------+     +------------------+
```

Decryption happens:
- During iPod sync (via iTunes + CoreFP.dll)
- On-device using Apple's Secure Enclave
- Results in EAPP files stored in `/iPod_Control/Games_RO/`

## Tooling

### Verification
```bash
# Check magic
xxd -l 4 file.bin | head -1
# Should show: 65617070 (eapp)

# Full header
xxd -l 40 file.bin
```

### Parsing
See `scripts/analyze_eapp.py` (to be created) for automated analysis.

## References

- `docs/DECRYPTION_ANALYSIS.md` - DRM analysis
- `docs/FAIRPLAY_RESEARCH_FINDINGS.md` - Key extraction
- `docs/FairPlay_Technical_Specification.md` - Encryption details
