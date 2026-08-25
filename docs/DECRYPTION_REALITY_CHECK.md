# FairPlay Decryption Reality Check

## What We Know

### AES-CTR Structure Confirmed
```
Ciphertext = Plaintext ⊕ AES_encrypt(IV || counter, ContentKey)
```

We have:
- ✅ **Known plaintext** (decrypted files with eapp headers)
- ✅ **Known ciphertext** (encrypted files)
- ✅ **Known IVs** from .sinf files (16 bytes each)
- ✅ **Derived keystream** (enc ⊕ dec)

### Keystream = AES(IV, ContentKey) ?

**NO.** The keystream is NOT the raw AES output. In AES-CTR:

```
Block 0: keystream[0:16]  = AES_encrypt(IV || counter=0, ContentKey)
Block 1: keystream[16:32] = AES_encrypt(IV || counter=1, ContentKey)
Block 2: keystream[32:48] = AES_encrypt(IV || counter=2, ContentKey)
...
```

Each block uses a **different counter value**, so we only have **1 block** of AES output per IV.

## Why Brute Force Won't Work

To find the 16-byte ContentKey:
- Search space: 2^128 possibilities
- Even with quantum computers: ~2^64 operations (still infeasible)
- AES is designed to resist exactly this attack

## What About Related-Key Attacks?

For games with **same user ID** (0772a7b0):
- Bejeweled: KS1 = AES(IV1, **K**)
- Cubis2: KS2 = AES(IV2, **K**)
- Minigolf: KS3 = AES(IV3, **K**)
- Sims Bowling: KS4 = AES(IV4, **K**)
- Sims Pool: KS5 = AES(IV5, **K**)

**5 equations, 1 unknown (K)**... but AES is non-linear. No known attack uses this structure to recover K.

## Realistic Paths Forward

### Path 1: VM + iTunes (Recommended)
**Difficulty: Medium | Success: High**

Boot the preservation VM and let iTunes do the decryption:
1. VM has 19 authorized accounts
2. iTunes 12.6.5.3 + CoreFP.dll configured
3. Games are already "purchased" in the VM
4. Either:
   - Sync to an iPod and extract decrypted files
   - Hook iTunes to extract keys from memory
   - Use Windows file monitoring to capture decrypted files

### Path 2: SC Info Key Extraction
**Difficulty: High | Success: Medium**

Extract the FairPlay account key from SC Info:
1. Derive correct device key from VM hardware (we have 5 candidates)
2. Decrypt SC Info files (AES-encrypted with device key)
3. Extract RSA private key for the user ID group
4. Unwrap priv blobs to get content keys
5. Decrypt all games with that user ID

**Blocker:** Need the exact device key derivation (BIOS/Processor string format)

### Path 3: Boot VM + Automated Extraction
**Difficulty: Medium | Success: High**

Use QEMU to boot VM, then:
1. Mount games directory via shared folder
2. Use Windows APIs or tools to trigger iTunes sync
3. Capture decrypted output
4. Script the process for all 54 games

## Current Recommendation

**Use Path 3: Boot VM with QEMU**

The preservation release includes working QEMU configs. Booting the VM is the most reliable way to access FairPlay decryption without reverse-engineering the entire crypto chain.

## Files Needed for Path 3

```
iPod Clickwheel Games Preservation Project Release 16/
├── start_qemu.sh           # QEMU launcher
├── qemu/                   # QEMU binaries and firmware
└── iPod Clickwheel Games Preservation Project.utm/
    └── Data/
        └── A973B7BF-F17A-44C5-A6D7-B6D819938FDC.qcow2  # VM disk
```

## Why This Works

- VM has all 19 accounts pre-authorized
- SC Info files are "hot" (device key matches VM hardware)
- iTunes will decrypt games during sync to iPod
- We can modify the VM or use file hooks to capture decrypted output

## Alternative: Don't Decrypt

Consider whether decryption is actually needed:
- Emulator could load encrypted games + .sinf, delegate to FairPlay
- But this requires CoreFP.dll integration (complex)
- Or use the VM as a "decryption appliance" (simpler)
