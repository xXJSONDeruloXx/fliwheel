# Tetris Save File Format Analysis

**Source:** Real iPod save files extracted from `/Volumes/KURT'S IPOD/iPod_Control/GameData_RW/66686/`

## Save Files

| File | Size | Purpose | Magic |
|------|------|---------|-------|
| `game.sav` | 3,561 bytes | Main game save | `MGCT` |
| `prefs.sav` | 127 bytes | User preferences | `RPCT` |
| `stats` | 168 bytes | Statistics/high scores | N/A |

---

## game.sav Format (Main Save)

### Header

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x00 | 4 | magic | `MGCT` | "MGCT" magic header |
| 0x04 | 4 | version | `0x00000001` | Save format version 1 |
| 0x08 | 4 | unk1 | `0x00000015` (21) | Unknown |
| 0x0c | 4 | unk2 | `0x00000021` (33) | Unknown |

### Score Data

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x10 | 4 | score | `0x000054f3` (21,747) | Current score |
| 0x14 | 4 | level | `0x00000002` (2) | Current level |
| 0x18 | 4 | lines | `0x0000008e` (142) | Lines cleared |
| 0x1c | 4 | unk3 | `0xffffffff` (-1) | Unknown/placeholder |

### Game State

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x20-0x27 | 8 | reserved | `0x00` | Reserved/padding |
| 0x28 | 4 | unk4 | `0x000003e8` (1000) | Possibly max score limit |
| 0x2c | 4 | unk5 | `0x00000032` (50) | Possibly line goal/target |
| 0x30-0x3f | 16 | padding | `0x00` | Zero padding |

### Additional Data (0x40+)

The remainder of the file (3,561 - 64 = 3,497 bytes) appears to contain:
- Board state (current piece positions)
- Next piece queue
- Hold piece
- Statistics counters
- High score tables

**Note:** Many values at offset 0x64+ are `0xffff` (65535), likely indicating empty/unused entries.

---

## prefs.sav Format (Preferences)

### Header

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x00 | 4 | magic | `RPCT` | "RPCT" magic header |
| 0x04 | 4 | size | `0x0000000c` (12) | Record size? |

### Settings Records

Preferences appear to be stored as multiple 0x34-byte records:

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x08 | 4 | count | `0x00000001` (1) | Setting ID/count |
| 0x0c | 4 | enabled | `0x00000001` (1) | Boolean: enabled |
| 0x10 | 4 | value | `0x01320101` | Packed value (music on, sfx on, etc.) |
| 0x14 | 4 | unk | `0x00000000` | Unknown |
| 0x18 | 4 | unk | `0x00000000` | Unknown |
| 0x1c | 4 | unk | `0x34170000` | Possibly timestamp/version |

**Structure:** The file contains 3-4 records (music, sound effects, difficulty, etc.), each 0x34 bytes.

---

## stats File Format (Statistics)

### Structure

The stats file contains high score entries, each 0x38 (56) bytes:

| Offset | Size | Field | Value | Description |
|--------|------|-------|-------|-------------|
| 0x00 | 4 | rank | `0x00000000` | Score rank (1st, 2nd, 3rd) |
| 0x04 | 4 | score | `0x00000400` (1024) | Score value |
| 0x08 | 4 | unk | `0x00000000` | Unknown |
| 0x0c | 12 | initials | `K.R.T.\0` | Player initials (UTF-16LE: `4b005200540000`) |
| 0x18 | 4 | timestamp? | `0x20e420e4` | Date/time or checksum |
| 0x1c | 4 | unk | `0xa555632c` | Unknown |
| 0x20 | 4 | unk | `0x18a2` | Unknown |
| 0x24-0x34 | varies | padding | - | Reserved |
| 0x38 | 4 | magic | `0xcafabeba` | End marker "\xba\xca\xfe\xba" (little endian) |

### High Score Entries

File contains 3 high score entries (168 bytes / 56 bytes = 3 entries):

**Entry 1 (Rank 1):**
- Score: 1,024
- Initials: K.R.T.

**Entry 2 (Rank 2):**
- Score: 8,192 (0x2000)
- Initials: K.R.T.

**Entry 3 (Rank 3):**
- Score: 8,192 (0x2000)
- Initials: K.R.T.

---

## FairPlay Metadata

**File:** `Tetris_1_1_3168325.bin.sinf`

Same format as previously analyzed, confirming:

| Field | Value |
|-------|-------|
| IV | `9a86c0164a1d7941c51176df35f1d60d` |
| User ID | `3c282d58` (Tetris group) |
| Purchaser | "Heather Brumley" |

---

## Save Format Summary

```
SAVE STRUCTURE
==============

game.sav (MGCT)
├── Header (64 bytes)
│   ├── Magic: "MGCT"
│   ├── Version: 1
│   ├── Score: 21,747
│   ├── Level: 2
│   └── Lines: 142
├── Board State
├── Piece Queue
├── Statistics
└── High Scores (possibly embedded)

prefs.sav (RPCT)
├── Header (12 bytes)
├── Music Settings (52 bytes)
├── Sound FX Settings (52 bytes)
├── Difficulty Settings (52 bytes)
└── Additional Settings

stats
├── High Score 1 (56 bytes)
├── High Score 2 (56 bytes)
└── High Score 3 (56 bytes)
```

---

## Emulator Implementation Notes

### Save File Loading

```c
// game.sav header
struct GameSaveHeader {
    char magic[4];      // "MGCT"
    uint32_t version;   // 1
    uint32_t unk1;      // 21
    uint32_t unk2;      // 33
    uint32_t score;     // Current score
    uint32_t level;     // Current level
    uint32_t lines;     // Lines cleared
    uint32_t unk3;      // -1 (placeholder)
    uint8_t reserved[8];
    uint32_t unk4;      // 1000
    uint32_t unk5;      // 50 (line goal?)
    uint8_t padding[16];
};
```

### Preferences Loading

```c
// prefs.sav header
struct PrefsHeader {
    char magic[4];      // "RPCT"
    uint32_t size;      // 12
};

// Settings record
struct SettingRecord {
    uint32_t id;        // Setting ID
    uint32_t enabled;   // Boolean
    uint32_t value;     // Packed settings
    uint32_t unk[2];
    uint32_t timestamp; // Or version
    uint32_t data[8];   // Extended data
};
```

### Statistics Loading

```c
// High score entry
struct HighScoreEntry {
    uint32_t rank;          // 0, 1, 2 (0 = 1st place)
    uint32_t score;         // Score value
    uint32_t unk1;          // Unknown
    char16_t initials[6];   // UTF-16LE "KRT\0"
    uint32_t timestamp[2];  // Date/checksum
    uint32_t unk2[4];       // Unknown/padding
    uint32_t end_marker;    // 0xcafabeba
};
```

---

## File Locations on iPod

```
/iPod_Control/
├── GameData_RW/66686/        # Tetris game ID
│   ├── game.sav              # Main save (3,561 bytes)
│   └── prefs.sav             # Preferences (127 bytes)
└── GameStats_WO/66686/en/
    └── stats                 # High scores (168 bytes)
```

---

## Notes for Emulator Development

1. **Save format is game-specific** - Each game likely has unique save formats
2. **Multiple save files** - Separate files for game state, preferences, and statistics
3. **UTF-16LE strings** - Initials use wide characters
4. **Magic headers** - Each file type has identifying magic: `MGCT`, `RPCT`
5. **Fixed-size records** - Statistics use 56-byte fixed entries
6. **Board state** - Likely compressed/encoded in game.sav after header

## Preservation Value

These are **real user save files** from an actual iPod, providing authentic examples of:
- Save file structure
- High score data format
- User preferences storage
- FairPlay metadata associated with saves
