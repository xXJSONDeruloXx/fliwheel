# FairPlay Windows Device Key Derivation Algorithm

**Source:** `requiem/WindowsConfig.cc`  
**Discovered:** 2025-01-23  
**Status:** Critical finding for SC Info decryption

---

## Algorithm Overview

The Windows FairPlay device key ("machine identifier" / "MAC address equivalent") is derived from **4 hardware components** using **nested MD5 hashes**:

```
Final 6-byte ID = MD5( seed ∪ hash1 ∪ hash2 ∪ hash3 ∪ hash4 )[0:6]
```

Where each hash is `MD5(component)[0:4]` (first 4 bytes of MD5).

---

## Components (in order)

### 1. Seed String
```cpp
const char *seed = "cache-controlEthernet";  // 21 bytes
MD5_Update(&main_ctx, seed, 21);
```

### 2. C: Drive Volume Serial Number
```cpp
DWORD volume_serial_number;
GetVolumeInformation("C:\\", NULL, 0, &volume_serial_number, NULL, NULL, NULL, 0);

MD5_Init(&ctx);
MD5_Update(&ctx, &volume_serial_number, 4);  // Little-endian DWORD
hash1 = MD5_Final(&ctx)[0:4];
```

### 3. BIOS Version
```cpp
RegOpenKeyEx(HKEY_LOCAL_MACHINE, "HARDWARE\\DESCRIPTION\\System", ...);
RegQueryValueEx(hkey, "SystemBiosVersion", ...);

MD5_Init(&ctx);
MD5_Update(&ctx, bios_version, bios_version_size);  // Variable length string
hash2 = MD5_Final(&ctx)[0:4];
```

### 4. Processor Name String
```cpp
RegOpenKeyEx(HKEY_LOCAL_MACHINE, "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0", ...);
RegQueryValueEx(hkey, "ProcessorNameString", ...);

MD5_Init(&ctx);
MD5_Update(&ctx, processor_name, processor_name_size);  // Variable length string
hash3 = MD5_Final(&ctx)[0:4];
```

### 5. SC Info.txt Content (Key Identifier)
```cpp
// File: C:\ProgramData\Apple Computer\iTunes\SC Info\SC Info.txt
// Contains: "00425-00000-00002-AA046" (example)

read_file(product_id_filename, &product_id, &product_id_size);

MD5_Init(&ctx);
MD5_Update(&ctx, product_id, product_id_size);  // Includes newline if present
hash4 = MD5_Final(&ctx)[0:4];
```

**Fallback:** If SC Info.txt doesn't exist and running 32-bit on 64-bit Windows:
```cpp
// Use Windows ProductId from registry instead
RegOpenKeyEx(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", ...);
RegQueryValueEx(hkey, "ProductId", ...);
```

---

## Final Computation

```cpp
// Append all 4-byte hashes to main context
MD5_Update(&main_ctx, hash1, 4);
MD5_Update(&main_ctx, hash2, 4);
MD5_Update(&main_ctx, hash3, 4);
MD5_Update(&main_ctx, hash4, 4);

// Final 16-byte MD5
byte final_md5[16];
MD5_Final(&main_ctx, final_md5);

// Return first 6 bytes as "MAC address"
return final_md5[0:6];
```

---

## Our VM Values

From the preservation VM analysis:

| Component | Value | Notes |
|-----------|-------|-------|
| **Seed** | `"cache-controlEthernet"` | Fixed constant |
| **Volume Serial** | `0xc20e7ed7` (LE) / `0xd77e0ec2` (BE read) | NTFS C: drive |
| **BIOS Version** | Unknown | Need VM registry |
| **Processor** | Unknown | Need VM registry |
| **SC Info.txt** | `00425-00000-00002-AA046` | 24 bytes incl. newline? |

**Result:** Should produce the same 6-byte machine identifier that CoreFP expects.

---

## SC Info Decryption Flow

With the device key, the FairPlay chain continues:

```
Device Key (6 bytes) ─┐
SC Info files ────┤─▶ CoreFP.dll ─▶ Decrypted SC Info ─▶ FairPlay Account Key
                        │
                        └─▶ Per-file content keys from .sinf priv blobs
```

From `decrypt_track.cc`:
```cpp
// Initialize with device key and SC Info directory
init(mac_addr, keystoredir);

// mac_addr = 6-byte device key
// keystoredir = "C:\\ProgramData\\Apple Computer\\iTunes\\SC Info"
```

The `init()` function calls CoreFP exports:
- `WIn9UJ86JKdV4dM` - Handshake with FairPlay certificate
- `X46O5IeS` - Authentication
- `YlCJ3lg` with opcode `0xf4419e34` - Load SC Info with device key

---

## CoreFP.dll Exported Functions (from Requiem)

| Function | Purpose |
|----------|---------|
| `WIn9UJ86JKdV4dM` | Initial handshake, exchanges certificates |
| `X46O5IeS` | Complete authentication after handshake |
| `YlCJ3lg` | Main dispatch function for all operations |

### YlCJ3lg Opcodes

| Opcode | Purpose | Arguments |
|--------|---------|-----------|
| `0xf4419e34` | Initialize/load SC Info | mac_addr, keystoredir, token |
| `0x66eb8880` | Get challenge for track | track_id, sinf, uuid |
| `0xe02fb955` | Authenticate track | response, handle |
| `0xa458f619` | Get decryption context | track info |
| `0x86adbd76` | Decrypt data block | context, data, size |

---

## Testing the Algorithm

To verify we can derive the same device key:

1. **Extract VM registry values:**
   - BIOS Version: `HKLM\HARDWARE\DESCRIPTION\System\SystemBiosVersion`
   - Processor Name: `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\ProcessorNameString`

2. **Read SC Info.txt:**
   - File: `docs/sc_info/SC Info.txt`
   - Content: `00425-00000-00002-AA046\n` (25 bytes?)

3. **Compute with seed:**
   ```python
   import hashlib
   
   seed = b"cache-controlEthernet"
   vol_serial = bytes.fromhex("d77e0ec2")  # Little-endian as stored
   
   main_ctx = hashlib.md5(seed)
   
   # Component 1: Volume serial
   h1 = hashlib.md5(vol_serial).digest()[:4]
   main_ctx.update(h1)
   
   # ... repeat for other components
   
   device_key = main_ctx.digest()[:6]
   ```

---

## Next Steps

1. ✅ **Documented:** Windows device key derivation algorithm
2. **TODO:** Extract VM BIOS and Processor values from mounted registry
3. **TODO:** Implement algorithm and verify device key matches CoreFP expectation
4. **TODO:** Use device key + SC Info to initialize CoreFP decryption
5. **TODO:** Decrypt a game `.bin` file using extracted content key

---

## Files to Extract from VM

| Registry Path | Value | Purpose |
|---------------|-------|---------|
| `HKLM\HARDWARE\DESCRIPTION\System` | `SystemBiosVersion` | Component 3 |
| `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0` | `ProcessorNameString` | Component 4 |

These can be read from the mounted `SYSTEM` hive at:
```
/Volumes/Untitled/Windows/System32/config/SYSTEM
```

---

## References

- Source: `requiem/WindowsConfig.cc` lines 43-119
- Source: `requiem/decrypt_track.cc` lines 177-210 (init function)
- Related: `docs/sc_info/README.txt` - VM hardware identifiers
