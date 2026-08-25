# iPod Click Wheel Games Emulator Architecture

## Overview

This document describes the architecture for an emulator that can run iPod Click Wheel games, including handling both encrypted (original) and decrypted (preservation) game files.

## Game File States

### State 1: Encrypted (Distribution)
```
/path/to/game.ipg (iPod Game bundle)
    iTunesMetadata.plist  - Purchase info
    Payload/
        GameID/
            Executables/
                game_1_1_XXXXXXX.bin       - AES-128-CTR encrypted
                game_1_1_XXXXXXX.bin.sinf - FairPlay metadata (IV, key pointer)
            Assets/
                tex_*.bin, snd_*.bin (also encrypted)
```

### State 2: On-Device (iPod)
```
/iPod_Control/Games_RO/
    GameID/
        Executables/
            game_1_1_XXXXXXX.bin       - EAPP (decrypted)
            game_1_1_XXXXXXX.bin.sinf - Still present
```

### State 3: Decrypted (Preservation)
```
Executables/
    game_1_1_XXXXXXX.bin       - EAPP (decrypted, eapp header)
    (no .sinf needed for execution)
```

## Architecture Decision: Encrypted vs Decrypted Support

### Option A: Decrypted-Only Emulator (Recommended)
**Complexity:** Low | **Legal:** Clean | **Distribution:** Requires pre-decrypted games

```
[User] -> [Decrypted EAPP] -> [Emulator] -> [Runs Game]
```

**Pros:**
- Simple loader (direct EAPP parsing)
- No DRM dependencies
- Clean legal position (fair use preservation)
- Works with existing preservation dumps

**Cons:**
- Requires user to have decrypted games
- Cannot load original .ipg files

### Option B: Emulator + External Decryption
**Complexity:** Medium | **Legal:** Depends | **Distribution:** Complex

```
[User] -> [Encrypted EAPP] -> [Emulator]
                                      |
                                      v
                            [CoreFP.dll / iTunes]
                                      |
                                      v
                            [Decrypted EAPP] -> [Runs Game]
```

**Pros:**
- Can load original .ipg files
- Uses legitimate decryption path

**Cons:**
- Requires Windows environment or CoreFP port
- Complex setup for users
- Depends on Apple's DRM infrastructure

### Option C: Native Decryption (Not Feasible)
**Complexity:** Impossible | **Status:** Cryptographically infeasible

Implementing FairPlay decryption natively would require:
- Apple's RSA private keys (compromised in 2007, revoked)
- Per-account keys (SC Info decryption)
- Device-specific hardware keys

**Conclusion:** Cannot implement native FairPlay decryption.

## Recommended Architecture: Decrypted-Only with Helper Tools

### Core Emulator
```
+---------------------+
|  EAPP Loader        |  <- Parse eapp header, map to memory
+---------------------+
|  ARM CPU Core       |  <- ARMv5/ARM7 emulation (thumb-2 support)
+---------------------+
|  Click Wheel I/O    |  <- Menu, Select, Play, <<, >> buttons
+---------------------+
|  Audio Engine       |  <- AAC decoder, PCM mixing
+---------------------+
|  Graphics           |  <- 220x176 LCD, 2-bit grayscale
+---------------------+
|  iPod OS API        |  <- Syscall interface
+---------------------+
```

### Optional Helper: Decryption Appliance
Separate tool (not part of emulator):

```
[Encrypted Games] -> [Windows VM with iTunes] -> [Decrypted Output]
                            ^
                            |
                    [User provides credentials]
                    (or uses preservation VM)
```

This keeps the emulator clean while enabling full workflow.

## EAPP Loading Process

```python
def load_eapp(path):
    # 1. Read header (0x28 bytes)
    header = read(path, 0x28)
    
    # 2. Verify magic
    if header.magic != "eapp":
        raise InvalidFormat()
    
    # 3. Allocate memory at load_addr
    mem = allocate(header.load_addr, file_size)
    
    # 4. Copy header
    mem[header.load_addr:header.load_addr+0x28] = header
    
    # 5. Copy code starting at header.code_start
    code_offset = 0x28
    code_size = header.data_end - header.code_start
    mem[header.code_start:header.data_end] = read(path, code_offset, code_size)
    
    # 6. Zero BSS
    mem[header.bss_start:header.data_end] = b'\x00' * (header.data_end - header.bss_start)
    
    # 7. Set PC to entry point
    cpu.pc = header.code_start
    
    return mem, cpu
```

## Asset Loading

Assets (textures, audio) reference by filename:

```c
// Game code makes syscalls like:
load_texture("tex_title.bin");
play_sound("snd_click.bin");
```

**Emulator implementation:**
1. Maintain asset directory mapping
2. Load on-demand when game requests
3. Cache in memory for performance

## Syscall Interface

Games use Apple's iPod OS API. Emulator must implement:

| Category | Functions | Notes |
|----------|-----------|-------|
| Graphics | `DrawBitmap`, `FillRect`, `ScrollScreen` | 220x176, 2-bit |
| Audio | `PlayAAC`, `PlayPCM`, `MixAudio` | AAC decoding required |
| Input | `GetKeyState`, `PollButtons` | Click wheel mapping |
| Filesystem | `OpenFile`, `ReadFile`, `SeekFile` | Asset access |
| Memory | `malloc`, `free` | Heap management |
| System | `GetTime`, `Delay`, `Exit` | Utility functions |

## Click Wheel Input Mapping

Physical iPod -> Emulator mapping:

```
+-------------+
|   MENU      |  -> ESC or Menu key
+-------------+
|  +-------+  |
|  |       |  |
|<<| SELECT|>> | -> Left/Right/Up/Down + Enter
|  |       |  |
|  +-------+  |
|   /\\__/\\   | -> Scroll wheel (mouse wheel or Q/E)
|  /        \\  |
+-------------+
|  [ ][>]     |  -> Space (Play/Pause)
+-------------+
```

## Audio Pipeline

```
Game requests -> [AAC Decoder] -> [PCM Buffer] -> [Mixer] -> [Audio Output]
                (ffmpeg/libfaad)              (multiple    (PortAudio/
                 or bundled)                   sounds)      SDL)
```

## Graphics Pipeline

```
Game draws -> [220x176 2-bit buffer] -> [Upscaler] -> [SDL/OpenGL Display]
(220x176,                        (optional: integer
 4 shades                        scaling or CRT
 gray)                           filter)
```

## Development Phases

### Phase 1: Basic Loader (Weeks 1-2)
- EAPP header parsing
- Memory mapping
- ARM instruction decoding (Unicorn Engine or custom)
- Basic execution

### Phase 2: Syscalls (Weeks 3-4)
- Graphics (simple framebuffer)
- Input (keyboard mapping)
- File I/O (asset loading)

### Phase 3: Audio (Weeks 5-6)
- AAC decoding
- Audio mixing
- Synchronization

### Phase 4: Polish (Weeks 7-8)
- Click wheel feel
- Graphics upscaling
- Save states

## Dependencies

| Component | Options |
|-----------|---------|
| ARM CPU | Unicorn Engine, QEMU, or custom |
| AAC Decoding | FFmpeg, libfaad, or fdk-aac |
| Graphics | SDL2, SFML, or OpenGL |
| Audio | SDL2, PortAudio, or miniaudio |
| Build | CMake, Meson, or Makefile |

## Legal Considerations

### Emulator Code
✅ **Clean** - Writing an emulator is legal (clean room)

### Decrypted Games
✅ **Fair Use** - Preservation of abandonware
⚠️ **Not Distributable** - User must provide their own

### Decryption Tool
⚠️ **Gray Area** - Using preservation VM with existing accounts
❌ **Not Distributable** - Cannot bundle Apple's DRM keys

### Recommended Distribution
```
[Open Source]
    emulator/          <- Your code, MIT/GPL
    docs/              <- Specifications (this doc)
    tools/             <- Helper scripts (not decryption keys)
    
[User Provides]
    games/decrypted/   <- Their own decrypted games
    or
    games/ipg/ + VM/   <- Original + preservation VM
```

## Prior Art

- **iPodLinux**: Open source iPod firmware (no games)
- **Rockbox**: Alternative firmware (no game support)
- **Hacks**: Decrypted game research (2007-2008)
- **This Project**: First comprehensive documentation + potential emulator

## Conclusion

**Build a decrypted-only emulator** with optional helper tools for DRM handling. This provides:
- Clean, maintainable codebase
- No legal baggage
- Works with preservation community dumps
- Educational value
- Future-proof (doesn't depend on Apple's infrastructure)

The decryption problem is _solved_ by the preservation VM for those who need it.
