# iPod Click Wheel Filesystem Analysis

## Overview

Analysis of a real iPod with 51 click wheel games installed. This reveals the on-device structure, save formats, statistics tracking, and FairPlay integration.

## Directory Structure

```
/iPod_Control/
├── Device/
│   ├── SysInfo              # Device information (empty?)
│   ├── Preferences          # Device settings (binary)
│   ├── PlayCounts           # Game play statistics
│   ├── clock                # Clock settings
│   └── alarms               # Alarm settings
├── Games_RO/                # Read-only game files
│   ├── {GameID}/            # e.g., 88908, 12100, etc.
│   │   ├── Executables/     # Encrypted .bin files
│   │   ├── *.m4a            # Audio files
│   │   ├── *.ro             # Resource files
│   │   ├── *.raw.lcd5       # Splash screens
│   │   ├── Manifest.plist   # File manifest with SHA1 digests
│   │   └── iTunesArtwork    # Game icon
│   └── ... (51 game directories)
├── GameData_RW/             # Read-write save data
│   └── {GameID}/
│       ├── SaveState.sav    # Game save state
│       ├── Options.dat      # Game settings
│       ├── Profiles.dat     # User profiles
│       └── *.bin/*.dat      # Game-specific formats
├── GameStats_WO/            # Write-once statistics
│   └── {GameID}/
│       └── {lang}/stats      # Per-language stats
└── iTunes/
    ├── iTunesDB             # iTunes database
    ├── iTunesControl        # Control file (20MB)
    ├── IC-Info.sidb         # FairPlay client info (23KB)
    ├── iTunesPrefs.plist    # iTunes preferences
    └── Rentals.plist        # Rental information
```

## Critical Findings

### 1. Files are ENCRYPTED on iPod

Despite common belief, the iPod stores **encrypted files**, not decrypted EAPP:

```
Bejeweled_1_1_3367225.bin (on iPod):
  00000000: 9d95 38ab 87bb a1aa ...  (ENCRYPTED)

Expected if decrypted:
  00000000: 6561 7070 0010 0010 ...  (eapp)
```

**Implication:** iPod firmware decrypts in RAM at runtime using hardware key.

### 2. IC-Info.sidb (FairPlay Client Info)

```
Location: /iPod_Control/iTunes/IC-Info.sidb
Size: 23,396 bytes
Format: Encrypted binary (likely AES-encrypted FairPlay data)
```

This is the iPod equivalent of Windows' SC Info files, containing:
- Device certificate
- Account authorizations
- Wrapped keys for content decryption

Unlike Windows (SC Info.sidb/sidd/sidn/sidr), iPod uses a single `.sidb` file.

### 3. Manifest.plist Structure

Every game has a `Manifest.plist` with:

```xml
<dict>
  <key>BuildIdentifier</key>
  <string>1470110895-0ff071ad9fe4</string>
  <key>Files</key>
  <array>
    <dict>
      <key>DRM</key>
      <true/>              <!-- Encrypted files marked -->
      <key>Digest</key>
      <string>E296B9D93079F91E9A6DFE4B9EB2EFFE4FDD6D62</string>
      <key>Path</key>
      <string>Executables/hd_4614181.bin</string>
      <key>Size</key>
      <integer>831432</integer>
      <key>Verify</key>
      <true/>
    </dict>
  </array>
  <key>GUID</key>
  <string>...</string>
</dict>
```

**Key insights:**
- DRM flag indicates FairPlay encrypted
- SHA1 digest for file integrity verification
- BuildIdentifier tracks game version

### 4. Save File Formats

Games use different save formats:

#### Type A: Structured Binary (Bejeweled-style)
```
00000000: 5741 0000 0000 0000 a000 0000 0800 00ff  WA..............
00000010: 5741 01ff f3ff f1ff a800 0000 7c00 00ff  WA..........|...
```
- "WA" marker (game-specific?)
- Contains high scores, settings

#### Type B: String-based (Phase music game)
```
00000000: 0000 0000 8102 0000 9332 c30d 1000 0000  .........2......
00000010: 0101 0000 0000 0005 0000 000f 0000 0074  ...............t
00000020: 6f70 5f73 6f6e 675f 7363 6f72 6573 0101  op_song_scores..
```
- Contains "top_song_scores" key
- Key-value structure with lengths

#### Type C: UTF-16 Strings (Sims-style)
```
00000020: 0000 0001 014f 0057 004e 0045 0052 0000  .....O.W.N.E.R..
```
- Profile names in UTF-16LE
- "OWNER" appears to be default profile name

#### Type D: Fixed-size (Settings)
```
Options.dat (172 bytes):
  00000000: 0400 0000 0100 0000 0000 0000 0100 0000  ................
  00000010: 4b00 5200 5400 0000 0000 0000 0000 0000  K.R.T...........
```
- "KRT" = User initials/profile name
- Fixed-size structure for settings

### 5. Graphics Format (.raw.lcd5)

```
Minigolf.raw.lcd5 (138,256 bytes):
  00000000: 4001 0000 d800 0000 8002 0000 3536 354c  @...........565L
                                          ^^^^
                                          "565L" = RGB565 format
```

**Format analysis:**
- Header: 16 bytes
  - 0x00-0x03: Width (0x0140 = 320? or 0x1401 = 5121?)
  - 0x04-0x07: Height (0x00d8 = 216? or 0xd800 = 55296?)
  - 0x08-0x0b: Pitch/Size (0x0280 = 640 bytes per row)
  - 0x0c-0x0f: Magic "565L" (RGB565 Little-endian)
- Data: RGB565 pixel data (16-bit per pixel)

iPod 5G screen: 220x176 pixels
But LCD files may be 320x216 (scaled down?)

### 6. Resource Files (.ro)

```
bejeweled_game2_m1a.ro:
  00000000: 2e2e 2f2e 2e2f 2e2e 2f67 616d 6573 5f52  ../../../games_R
  00000010: 4f2f 6265 6a65 7765 6c65 645f 6761 6d00  O/bejeweled_gam.
  00000030: 0000 0000 5245 534f                      ....RESO
                                                ^^^^
                                                "RESO" = Resource marker
```

**Format:**
- "RESO" magic marker
- Contains file paths (Unix-style)
- Likely resource bundle (textures, tables, etc.)

### 7. Game Statistics Tracking

```
GameStats_WO/{GameID}/{lang}/stats:
  - WO = "Write Once" (append-only statistics)
  - Contains gameplay metrics
  - UTF-16LE strings with field names
  - Example: "Num Top Song Scores", "Games Played", "Best Half Round"
```

### 8. iTunes Integration

**iTunesPrefs.plist:**
```xml
<key>supportsGames</key>
<true/>
<key>totalGames</key>
<integer>2</integer>
<key>totalGameBytes</key>
<integer>5152768</integer>
```

Shows iTunes tracks game installation.

**PlayCounts:**
Binary database with game IDs and play statistics.

## Comparison: Windows vs iPod FairPlay

| Component | Windows | iPod |
|-----------|---------|------|
| SC Info | Multiple files (.sidb, .sidd, .sidn, .sidr) | Single IC-Info.sidb |
| Location | C:\ProgramData\Apple Computer\iTunes\SC Info | /iPod_Control/iTunes/ |
| CoreFP | CoreFP.dll | Embedded in firmware |
| Decryption | iTunes decrypts during sync | iPod decrypts at runtime |
| Storage | Encrypted on PC, decrypted on iPod | Encrypted on iPod |

## Implications for Emulator

### 1. Cannot Extract Decrypted Files from iPod
- Files are encrypted on iPod storage
- Decryption happens in RAM via firmware
- Hardware key required

### 2. Save File Emulation
- Multiple save formats to support
- Need to reverse engineer each game's format
- High scores, settings, progress tracking

### 3. Graphics Format
- RGB565 (.raw.lcd5) splash screens
- May need scaling (320x216 → 220x176)
- 16-bit color depth

### 4. Resource Loading
- .ro files contain bundled assets
- "RESO" format needs parser
- Path references suggest internal structure

### 5. Statistics Tracking
- Optional: track game usage like iPod
- Could export to GameStats_WO format for authenticity

## Save File Format Priorities for Emulator

High priority (most games use similar formats):
1. **Options.dat** - Settings (fixed 172-byte structure)
2. **SaveState.sav** - Generic save state
3. **Profiles.dat** - UTF-16LE profile names

Medium priority (game-specific):
4. **Phase-style** - Key-value with string keys
5. **Bejeweled-style** - Binary with "WA" marker
6. **.dtb format** - Structured binary tables

## Recommendations

1. **Use preservation VM** for decrypted game files
2. **Implement save file formats** based on reverse engineering
3. **Support RGB565 graphics** for splash screens
4. **Document .ro format** for complete asset loading
5. **Create save game editor** for user convenience

## Files of Interest

| File | Purpose | Size |
|------|---------|------|
| IC-Info.sidb | FairPlay client info | 23,396 bytes |
| Manifest.plist | Game file manifest | Variable |
| Options.dat | Game settings | 172 bytes |
| SaveState.sav | Save game state | Variable |
| *.raw.lcd5 | Splash screen | 138,256 bytes |
| *.ro | Resource bundle | Variable |
| PlayCounts | Usage statistics | 96 bytes |
