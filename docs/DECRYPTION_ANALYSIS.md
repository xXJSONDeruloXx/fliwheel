# iPod Click Wheel Game Decryption Analysis

## Overview
This document tracks the reverse engineering effort to decrypt iPod click wheel game executables (`.bin` files in `Games_RO/*/Executables/`). We have access to both encrypted versions (from original iPod firmware dumps) and decrypted versions (from the `16-ipod-games` repository), allowing us to perform known-plaintext analysis.

## File Structure

### Encrypted Source (Original iPod Dump)
```
/Users/kurt/Downloads/iPod Games Files/All iPods/iPod_Control/Games_RO/
├── {GameID}/
│   ├── Executables/
│   │   ├── {GameName}_{version}_{build}.bin          ← ENCRYPTED
│   │   └── {GameName}_{version}_{build}.bin.sinf     ← FairPlay DRM metadata
│   ├── Manifest.plist
│   └── {LaunchArt}.raw.lcd5
```

### Decrypted Reference (16-ipod-games Repo)
```
/Users/kurt/Downloads/16-ipod-games/Games_RO/
├── {GameID}/
│   ├── Executables/
│   │   └── {GameName}_{version}_{build}.bin          ← DECRYPTED (eapp header visible)
│   └── ...
```

## Key Findings

### 1. eapp Executable Format
Decrypted binaries start with a clear `eapp` magic header (4 bytes: `0x65 0x61 0x70 0x70`):

| Offset | Field | Description |
|--------|-------|-------------|
| 0x00 | magic | `"eapp"` (4 bytes) |
| 0x04 | load_addr | Typically `0x10001000` (4 bytes, little-endian) |
| 0x08 | format_version | Typically `5` (4 bytes) |
| 0x0C | header_size | Typically `0x28` / 40 bytes (4 bytes) |
| 0x10+ | other header words | 9 x 32-bit words total |
| 0x28 | first_code_word | ARM instruction (e.g., `0xEAFFFFFE` = branch) |

### 2. Encryption is Per-File Stream Cipher
- Each encrypted file has a **unique keystream** (XOR delta between encrypted/decrypted)
- Keystream does NOT repeat within a file (checked up to 4KB, no period found)
- Keystream has high entropy (252/256 byte values in first 1KB)
- Different games have completely different keystreams
- Asset files (textures, audio) are **not encrypted** (keystream = all zeros)

### 3. FairPlay DRM `.sinf` Structure
Each encrypted executable has a companion `.sinf` file using an MP4/FairPlay-style atom structure.

Top-level structure observed:
- `sinf` (container, 1032 bytes total in samples)
  - `frma` = `game`
  - `schm` = `itun`
  - `schi` (scheme info)
    - `user` (4 bytes)
    - `key ` (4 bytes)
    - `iviv` (16 bytes)
    - `righ` (80 bytes)
    - `name` (256 bytes)
    - `priv` (440 bytes)
  - `sign` (128 bytes)

Observed atom meanings:

| Atom | Content | Size | Notes |
|------|---------|------|-------|
| `sinf.schi.user` | User / key-set ID | 4 bytes | Not constant across all games |
| `sinf.schi.key ` | Key indicator | 4 bytes | Seen values: `0x00000003`, `0x00000004`, `0x00000006` |
| `sinf.schi.iviv` | **Initialization Vector** | 16 bytes | Unique per file |
| `sinf.schi.righ` | Rights metadata | 80 bytes | Contains structured metadata, not yet decoded |
| `sinf.schi.name` | Name string field | 256 bytes | Example: `Colton Raithel` in one sample |
| `sinf.schi.priv` | **Encrypted key blob / key bag** | 440 bytes | High entropy; likely wrapped per-file content key |
| `sinf.sign` | Signature | 128 bytes | Likely RSA-1024 signature over DRM payload |

Notes:
- `priv` blobs are different for every sampled file, even when `user` is the same.
- `priv` does **not** parse cleanly as plain ASN.1 DER.
- Same-user games likely share a higher-level key hierarchy, but still carry distinct per-file wrapped content keys.

### 4. Extracted `.sinf` Values

| Game | Build | User | Key Atom | IV (16 bytes) |
|------|-------|------|----------|---------------|
| Bejeweled (12100) | 3367225 | `0772a7b0` | `00000004` | `e9 3b 50 ad fa 14 33 76 e2 cc 27 13 a4 b2 2d 51` |
| Cubis2 (13100) | 3296552 | `0772a7b0` | `00000004` | `bc 44 ef 62 48 42 70 51 22 db aa 12 89 cb c4 85` |
| Sims Bowling (15032) | 3214522 | `0772a7b0` | `00000004` | `5a d9 0d ae 41 2e eb ee 2f f9 52 78 d1 76 3e 61` |
| Sims Pool (15035) | 3214516 | `0772a7b0` | `00000004` | `cc c3 d4 4b 8b 4b 43 66 04 68 67 c1 0e 6a 0d f7` |
| Sudoku (50533) | 3146361 | `0ce22957` | `00000003` | `cf d1 e8 53 8a 6e e8 38 93 3f 21 00 c3 36 f0 bc` |
| Tetris (66686) | 3168325 | `3c282d58` | `00000006` | `9a 86 c0 16 4a 1d 79 41 c5 11 76 df 35 f1 d6 0d` |
| Minigolf (88908) | 3299468 | `0772a7b0` | `00000004` | `e3 2b 83 99 a5 b7 23 61 9f 16 a9 b4 4d ef e0 12` |

Takeaways:
- Five sampled games share `user=0772a7b0` and `key=00000004`.
- Tetris and Sudoku use different user/key identifiers, implying at least 3 distinct DRM key groups in the sample set.

### 5. Keystream First Words (Known Plaintext Attack)

| Game | Build | KS[0] | KS[1] | KS[2] | KS[3] |
|------|-------|-------|-------|-------|-------|
| Bejeweled | 3367225 | `0xDB48F4F8` | `0xBAA1AB87` | `0x4CF72401` | `0x3687F277` |
| Cubis2 | 3296552 | `0x2FC782C7` | `0xF22BB006` | `0xA84A8F47` | `0x3006E0E8` |
| Tetris | 3168325 | `0xF51C11E0` | `0x82F34E54` | `0x40A47BF5` | `0x194C4B27` |
| Minigolf | 3299468 | `0x48A3FD72` | `0xAC9FE088` | `0xB6BF2DF9` | `0x19E70DC1` |
| Sims Bowling | 3214522 | `0xCF1EBD1A` | `0xA010D959` | `0xBBCD0810` | `0xEED63767` |
| Sims Pool | 3214516 | `0xA8053056` | `0x7BB091EF` | `0xC1F82AFD` | `0x56FD56F0` |
| Sudoku | 3146361 | `0x03415595` | `0xE80A27DB` | `0x5E0EBB66` | `0xC763046B` |

## Hypothesis: AES-CTR-Like Content Encryption
The evidence still strongly suggests an AES-CTR-style design:
- 16-byte IV per file matches AES block size
- Encryption is length-preserving with no padding
- Each file has a unique, high-entropy keystream
- `.sinf` carries per-file IV plus a wrapped high-entropy `priv` blob
- `user` / `key ` fields imply multiple DRM key groups

Current confidence: **moderate**

What is solid:
- This is not a fixed XOR key.
- This is not a repeating-key stream.
- Executables are protected per-file.
- `.sinf` almost certainly carries the metadata needed to unwrap the real content key.

What is not yet proven:
- Exact cipher/mode (AES-CTR is still the leading guess, not yet verified cryptographically).
- Exact meaning of `key ` values `3/4/6`.
- How `priv` is wrapped and what root/material is needed to unwrap it.

## 6. FairPlay DRM Key Material Located (Preservation VM)

The **iPod Clickwheel Games Preservation Project Release 16** archive contains a
UTM/QEMU Windows VM (`IK-PC`) with iTunes 12.6.5.3 installed and **all accounts
authorized**. This VM holds the complete FairPlay key chain needed to decrypt
every game in the set.

### Decryption Chain (documented architecture)

```
  .sinf priv blob ──┐
  .sinf iviv       │
  .sinf user       ├──► iTunes + CoreFP.dll ──► AES-128 content key + IV
  SC Info files ───┤                            │
  adi.pb ──────────┘                            ▼
                                         AES-CTR decrypt .bin
```

1. iTunes derives a **device key** from the Windows hardware ID
2. Device key decrypts **SC Info** key database → FairPlay account key
3. FairPlay account key unwraps **`priv`** blob → per-file AES-128 content key
4. Content key + **`iviv`** → AES-CTR stream cipher → decrypts the `.bin` executable

### Extracted VM key material

All copied to `docs/sc_info/`:

| File | Size | Purpose |
|------|------|---------|
| `SC Info.sidb` | 231 KB | FairPlay key database (encrypted) |
| `SC Info.sidd` | 4.2 MB | FairPlay key data / key bag (encrypted) |
| `SC Info.sidn` | 88 KB | Additional key data (encrypted) |
| `SC Info.sidr` | 1.2 KB | Rights info (encrypted) |
| `SC Info.txt` | 24 B | `00425-00000-00002-AA046` (key identifier) |
| `CoreFP.dll` | 31.6 MB | FairPlay decryption engine (from `Program Files\iTunes\`) |
| `adi.pb` | 510 B | Apple Device Identity (v1) |
| `adi-2CC3E287.pb` | 435 B | Apple Device Identity (v3) |
| `accounts/*/` | various | Per-account SC Info snapshots (19 accounts) |

### VM Hardware Identity (needed to decrypt SC Info)

| Identifier | Value |
|------------|-------|
| MachineGuid (registry) | `7eaac238-4170-4f15-8bf0-bf4bc727cf39` |
| ComputerName | `IK-PC` |
| NTFS Volume Serial | `0xc20e7ed7` (LE: `d7 7e 0e c2`) |
| iTunes Version | `12.6.5.3` (64-bit) |
| Windows | LTSC 2019 (x64) |

Simple SHA1/MD5 derivations of MachineGuid **do not** decrypt the SC Info files
in ECB. The FairPlay 2 device-key derivation inside CoreFP.dll uses a more
complex algorithm that has not yet been reverse-engineered.

### Account → Email → Game mapping (19 accounts)

Per-account SC Info backups exist at
`C:\Users\IK\Desktop\SC Info Backup\{N}\`, each containing the FairPlay key
snapshot for that authorization state. Email and game mapping:

| Backup | Email | Representative games |
|--------|-------|---------------------|
| 1 | byrdparenzin@yahoo.com | Texas Hold'em |
| 3 | alineup@free.fr | (various) |
| 4–5 | kylezhou2002@* | (various) |
| 6 | olivianicolegreer1995@hotmail.com | Monopoly, Peggle |
| 7 | hbrumley@odessa.edu | Tetris |
| 8 | toncol12@aol.com | Bejeweled, Cubis2, Sims Bowling/Pool/DJ |
| 9 | trishgen@ca.rr.com | Ms. PAC-MAN, Scrabble |
| 10 | dmn.001@gmail.com | Chinese Checkers, Tamagotchi |
| 13 | m4tt94@gmail.com | UNO, Asphalt4, Naval Battle, etc. |
| 14 | vortis-1@mx5.canvas.ne.jp | (various) |
| 16 | bicycle.dude@yahoo.com | Sudoku |
| 17 | e.waanders@well.speedlinq.nl | (various) |
| 18 | stachofsky_2@q.com | Tiger Woods |
| 19 | appenzeller@earthlink.net | Real Soccer 2009 |

The `latest/` folder contains the merged SC Info for all accounts currently
authorized on the VM.

### Purchase metadata from `.ipg` files

The `iTunes Games Files/All iPods/*.ipg` bundles are original iTunes Store
purchases (ZIP archives). Each contains:
- `Manifest.plist` — signed game manifest with file SHA1 digests and `DRM: true` flags on executables
- `Manifest.plist.p7b` — PKCS#7 detached signature
- `iTunesMetaData` — MP4 metadata with purchaser email (`apID`), purchase date (`purd`), etc.
- `Executables/*.bin` + `*.bin.sinf` — encrypted binary + FairPlay key

## Next Steps

1. **Reverse-engineer CoreFP.dll** to find the exact device-key derivation function (SHA1/MD5 of hardware ID + salt/transform) that decrypts SC Info.
2. **Alternatively boot the VM** via UTM/QEMU and extract decrypted binaries or content keys at runtime (via debugger or by syncing to a device).
3. **Build a standalone FairPlay 2 decryptor** once the device-key derivation is understood.
4. **Test AES-CTR decryption** with extracted content key + IV against known plaintext.
5. **Document exact unwrap/decrypt flow** in this repo.

## Current Status

Solved and documented:
- Known encrypted/decrypted executable pairs exist and are byte-comparable.
- Decrypted files consistently expose valid `eapp` headers.
- Executable protection is per-file, high-entropy stream cipher.
- `.sinf` sidecar files contain per-file IVs + wrapped content keys.
- **All FairPlay key material located**: SC Info, CoreFP.dll, adi.pb, hardware ID.
- Per-account key snapshots preserved for all 19+ purchasing accounts.

Not yet solved:
- The FairPlay 2 device-key derivation algorithm (inside CoreFP.dll).
- Unwrapping `priv` into the actual content key without running iTunes.
- Producing a fresh successful decrypt from encrypted input + `.sinf` + SC Info alone.

## Tools & Scripts

- `scripts/games/ipod_games_probe.py` — Parses game bundles and eapp headers
- `docs/sc_info/` — Extracted FairPlay key material from the preservation VM
- Custom Python scripts for XOR/keystream analysis (documented in this repo)

## Related Files in Repo
- `docs/IPOD_GAMES_ALL_IPODS_TESTRESULTS.md` — Full inventory of 54 games tested
- `docs/DECRYPTION_ANALYSIS.md` — This file
- `docs/sc_info/` — SC Info, CoreFP.dll, adi.pb, per-account key snapshots
