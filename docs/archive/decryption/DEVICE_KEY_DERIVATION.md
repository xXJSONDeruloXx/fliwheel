# FairPlay Device Key Derivation for Preservation VM

## Summary

Using the algorithm discovered in Requiem's `WindowsConfig.cc`, we derived the **device key** from 4 hardware components. This 6-byte key ("MAC address equivalent") is required to initialize CoreFP.dll and decrypt SC Info files.

## Algorithm

```
Device Key = MD5( SEED ∪ hash1 ∪ hash2 ∪ hash3 ∪ hash4 )[0:6]

Where:
  SEED = "cache-controlEthernet" (21 bytes)
  hash1 = MD5(Volume Serial LE)[0:4]
  hash2 = MD5(BIOS Version)[0:4]
  hash3 = MD5(Processor Name)[0:4]
  hash4 = MD5(SC Info.txt content)[0:4]
```

## Extracted VM Hardware Values

### 1. Volume Serial Number (C: drive)
- **Source:** NTFS boot sector at offset 0x48
- **Value:** `0xc20e7ed7` (little-endian DWORD)
- **Bytes:** `d7 7e 0e c2`

### 2. BIOS Version
- **Source:** `SYSTEM\HardwareConfig\{...}\BIOSVersion`
- **Discovered Value:** `0.0.0`
- **Alternative:** Full firmware string `EFI Development Kit II / OVMF 0.0.0`
- **Encoding Uncertainty:** ASCII vs UTF-16LE, with/without null terminator

### 3. Processor Name String
- **Source:** `SYSTEM\ControlSet001\Control\Session Manager\Environment\PROCESSOR_IDENTIFIER`
- **Discovered Value:** `Intel64 Family 6 Model 94 Stepping 3, GenuineIntel`
- **Encoding Uncertainty:** ASCII vs UTF-16LE, with/without null terminator

### 4. SC Info.txt Content
- **Source:** `docs/sc_info/SC Info.txt`
- **Value:** `00425-00000-00002-AA046\x00` (24 bytes with null terminator)

## Candidate Device Keys

| Candidate | BIOS Format | Processor Format | Device Key (hex) |
|-----------|-------------|------------------|------------------|
| 1 | ASCII "0.0.0" | ASCII no null | `042effb65443` |
| 2 | ASCII + null | ASCII + null | `9626413d43b7` |
| 3 | Full firmware | ASCII | `8699214fc26f` |
| 4 | ASCII + null | UTF-16LE + null | `d519b2aa0568` |
| 5 | BIOSVendor | ASCII | `0b539dda7406` |

## Key Usage in CoreFP

From `decrypt_track.cc` in Requiem:

```c
// Initialize FairPlay with device key
byte mac_addr[6] = { 0x04, 0x2e, 0xff, 0xb6, 0x54, 0x43 }; // candidate 1

// Call YlCJ3lg with opcode 0xf4419e34 to load SC Info
struct {
    uint32_t size;
    byte data[6];
} mac = { 6, mac_addr[0], mac_addr[1], ... };

uint32_t args[5] = {1, 0, (uint32_t)&mac, (uint32_t)keystoredir, (uint32_t)&token4};
YlCJ3lg_call(0xf4419e34, 5, args);
```

## Next Step: Test Candidates

### Option 1: Windows Test Program
Create a minimal Windows executable that:
1. Loads CoreFP.dll from preservation VM
2. Tries each device key candidate
3. Reports which one successfully initializes (returns 0 from YlCJ3lg_call)

### Option 2: Boot Preservation VM
Since the VM has working iTunes, we can:
1. Extract device key from running system (memory, debug iTunes)
2. Compare with our candidates
3. Confirm the correct derivation

### Option 3: Cross-Reference with Requiem
The Requiem repository includes `CoreFPWin-2.2.19/CoreFP.dll` (older version).
If we can extract the working device key from the VM environment, we can:
1. Test against our candidates
2. Determine the correct string formats
3. Apply to newer CoreFP.dll (12.6.5.3)

## C Code for Testing

```c
#include <windows.h>
#include <stdio.h>
#include <wincrypt.h>

#pragma comment(lib, "advapi32.lib")

// MD5 helper
typedef struct {
    DWORD state[4];
    DWORD count[2];
    BYTE buffer[64];
} MD5_CTX;

void MD5_Init(MD5_CTX* ctx);
void MD5_Update(MD5_CTX* ctx, const BYTE* data, UINT len);
void MD5_Final(BYTE digest[16], MD5_CTX* ctx);

// Test device key derivation
void derive_device_key(BYTE* key_out) {
    const BYTE seed[] = "cache-controlEthernet";
    BYTE volume_serial[] = { 0xd7, 0x7e, 0x0e, 0xc2 }; // 0xc20e7ed7 LE
    
    // Test variations of BIOS and Processor
    const BYTE* bios = (const BYTE*)"0.0.0";
    size_t bios_len = 5;
    
    const BYTE* processor = (const BYTE*)"Intel64 Family 6 Model 94 Stepping 3, GenuineIntel";
    size_t proc_len = 50;
    
    // Read SC Info.txt
    BYTE sc_info[100];
    DWORD sc_info_len = 0;
    HANDLE hFile = CreateFile(
        "C:\\ProgramData\\Apple Computer\\iTunes\\SC Info\\SC Info.txt",
        GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
    if (hFile != INVALID_HANDLE_VALUE) {
        ReadFile(hFile, sc_info, sizeof(sc_info), &sc_info_len, NULL);
        CloseHandle(hFile);
    }
    
    // Compute device key
    MD5_CTX ctx, inner;
    MD5_Init(&ctx);
    MD5_Update(&ctx, seed, strlen((char*)seed));
    
    // Hash1: Volume serial
    BYTE hash1[4];
    MD5_Init(&inner);
    MD5_Update(&inner, volume_serial, 4);
    MD5_Final(hash1, &inner);
    MD5_Update(&ctx, hash1, 4);
    
    // Hash2: BIOS
    BYTE hash2[4];
    MD5_Init(&inner);
    MD5_Update(&inner, bios, bios_len);
    MD5_Final(hash2, &inner);
    MD5_Update(&ctx, hash2, 4);
    
    // Hash3: Processor
    BYTE hash3[4];
    MD5_Init(&inner);
    MD5_Update(&inner, processor, proc_len);
    MD5_Final(hash3, &inner);
    MD5_Update(&ctx, hash3, 4);
    
    // Hash4: SC Info
    BYTE hash4[4];
    MD5_Init(&inner);
    MD5_Update(&inner, sc_info, sc_info_len);
    MD5_Final(hash4, &inner);
    MD5_Update(&ctx, hash4, 4);
    
    // Final
    BYTE final[16];
    MD5_Final(final, &ctx);
    memcpy(key_out, final, 6);
}

// Load CoreFP and test initialization
int test_corefp(const BYTE* device_key) {
    HMODULE hCoreFP = LoadLibrary("CoreFP.dll");
    if (!hCoreFP) return -1;
    
    FARPROC YlCJ3lg = GetProcAddress(hCoreFP, "YlCJ3lg");
    if (!YlCJ3lg) return -2;
    
    // ... call initialization with device_key
    // Return 0 on success
    return 0;
}

int main() {
    BYTE key[6];
    derive_device_key(key);
    printf("Derived device key: %02x%02x%02x%02x%02x%02x\n",
           key[0], key[1], key[2], key[3], key[4], key[5]);
    
    int result = test_corefp(key);
    printf("CoreFP init result: %d\n", result);
    
    return 0;
}
```

## Files for Testing

| File | Location | Purpose |
|------|----------|---------|
| CoreFP.dll | `docs/sc_info/CoreFP.dll` | FairPlay decryption engine |
| SC Info files | `docs/sc_info/SC Info.*` | Encrypted key database |
| Device key candidates | This document | Test each candidate |

## Success Criteria

When the correct device key is used:
1. `YlCJ3lg` call with opcode `0xf4419e34` returns 0
2. Token4 is populated (non-zero)
3. Subsequent calls to `init_track()` succeed
4. Game `.bin` files can be decrypted

## References

- Source: `requiem/WindowsConfig.cc` (device key derivation)
- Source: `requiem/decrypt_track.cc` (CoreFP initialization)
- Document: `docs/FAIRPLAY_WINDOWS_DERIVATION.md` (full algorithm)
