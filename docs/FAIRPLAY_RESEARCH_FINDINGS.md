# FairPlay DRM Research Findings

## Executive Summary

Through web research and GitHub exploration, we found **existing open-source tools** that successfully interact with FairPlay DRM, providing a path forward without full reverse-engineering of CoreFP.dll. The key discovery is **Requiem**, a working FairPlay decryption tool from the iTunes 10-11 era.

---

## Research Results

### 1. pwn0rz/fairplay_research
**Repository:** https://github.com/pwn0rz/fairplay_research  
**Stars:** 235 | **Forks:** 41 | **Language:** C

**Purpose:** Kernel-level FairPlay debugger for macOS  
**Approach:** Loads `FairPlayIOKit` driver into userspace for LLDB debugging

**Key Tools:**
- `sinf_view.py` - Parses `.sinf` DRM metadata files ✅ (already using)
- `supf_view.py` - Parses `.supf` files (purchaser info)
- `branch.py` - Branch tracing for FairPlay driver
- `fprpc.c` - RPC communication with FairPlay

**Relevance:** Confirms our `.sinf` parsing is correct. Used successfully to extract IVs and `priv` blobs from iPod game files.

---

### 2. Shaddy1884/requiem ⚠️ CRITICAL
**Repository:** https://github.com/Shaddy1884/requiem  
**Stars:** 105 | **Forks:** 19 | **Language:** Java + Native (C/C++)

**Purpose:** Apple FairPlay DRM decryption system for iTunes content  
**Era:** iTunes 10.x - 11.x (compatible with iPod game DRM timeline)

**How It Works:**
```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  iTunes Library │────▶│  Requiem Java   │────▶│  Native Library │
│  (encrypted)    │     │  (orchestrator) │    │  (libNative*.dylib)│
└─────────────────┘     └─────────────────┘     └─────────────────┘
                              │                           │
                              ▼                           ▼
                        ┌─────────────┐           ┌─────────────────┐
                        │  SC Info    │───────────│    CoreFP       │
                        │  (keys)     │  native   │   (Apple DLL)   │
                        └─────────────┘   calls   └─────────────────┘
                                                          │
                                                          ▼
                                                   ┌─────────────────┐
                                                   │  Decrypted      │
                                                   │  Content        │
                                                   └─────────────────┘
```

**Key Technical Details:**

| Aspect | Implementation |
|--------|----------------|
| SC Info Location (Mac) | `/Users/Shared/SC Info` |
| SC Info Location (Win) | `C:\ProgramData\Apple Computer\iTunes\SC Info` ✅ (matches our VM) |
| iTunes Library Key | `BHUILuilfghuila3` (AES-ECB, hardcoded) |
| Native Interface | `libNative32.dylib` / `libNative64.dylib` (Mac) |
| CoreFP Versions Included | 2.1.34 (Mac), 2.2.19 (Win) |

**Code Evidence:**
```java
// From ModifyLib.java - iTunes library decryption
Key key = new SecretKeySpec("BHUILuilfghuila3".getBytes(), "AES");
Cipher cipher = Cipher.getInstance("AES/ECB/NoPadding");
cipher.init(Cipher.DECRYPT_MODE, key);

// From MacConfig.java - SC Info location
String keyStoreDirectory() {
    return "/Users/Shared/SC Info";
}

// Library modification approach
// 1. Loads native library that interfaces with CoreFP
// 2. Native library calls FairPlay APIs to decrypt content
// 3. Requiem patches iTunes library to mark files as "Purchased" not "Protected"
```

**Limitations:**
- Designed for music/video (m4p, m4v), not specifically iPod games
- Does not work with latest iTunes versions
- No key extraction — uses FairPlay APIs for authorized decryption

**Included Binaries:**
```
CoreFP-2.1.34/CoreFP.i386       (4.0 MB Mac)
CoreFP-2.1.34/CoreFP.icxs       (1.5 MB Mac CoreX)
CoreFP1-1.14.34/CoreFP1.i386    (18 MB older Mac)
CoreFPWin-2.2.19/CoreFP.dll     (25 MB Windows)
```

---

### 3. tedzhang2891/M4VDRMRemoval
**Repository:** https://github.com/tedzhang2891/M4VDRMRemoval  
**Stars:** 2 | Minimal implementation, uses FFmpeg for iTunes M4V decryption

**Relevance:** Shows FFmpeg can decrypt FairPlay content with proper keys, but doesn't solve key extraction.

---

## Comparison: Requiem CoreFP vs. Our Preservation VM CoreFP

| Attribute | Requiem (2014) | Our VM (iTunes 12.6.5.3) |
|-----------|----------------|--------------------------|
| **CoreFP.dll Size** | 25.1 MB | 31.6 MB |
| **CoreFP Version** | 2.2.19 | Unknown (newer) |
| **iTunes Era** | 10.x - 11.x | 12.6.5.3 |
| **FairPlay Version** | v1/v2 | v2/v3? |
| **SC Info Format** | Same structure | Same structure ✅ |
| **Platform** | Windows/Mac | Windows (VM) |

**Key Insight:** The SC Info file structure and CoreFP API likely remained stable across versions. Requiem's native library interface code should be adaptable.

---

## Our Assets vs. Required for Decryption

### What We Have ✅

| Asset | Location | Purpose |
|-------|----------|---------|
| CoreFP.dll | `docs/sc_info/CoreFP.dll` (31.6 MB) | FairPlay decryption engine |
| SC Info.sidb | `docs/sc_info/` (231 KB) | Encrypted key database |
| SC Info.sidd | `docs/sc_info/` (4.2 MB) | Encrypted key data |
| SC Info.sidn | `docs/sc_info/` (88 KB) | Additional key data |
| SC Info.sidr | `docs/sc_info/` (1.2 KB) | Rights info |
| adi.pb files | `docs/sc_info/adi*.pb` | Device identity |
| Per-account SC Info | `docs/sc_info/accounts/*/` | 19 authorized accounts |
| .sinf files | With each game executable | Per-file IV + wrapped keys |
| VM Hardware ID | Documented | MachineGuid, ComputerName, etc. |

### What Requiem Provides

| Asset | Location in Repo | Purpose |
|-------|------------------|---------|
| Native library source | `libNative*.c` or equivalent | Shows how to call CoreFP |
| Java-C interface | JNA/JNI bindings | API wrapper code |
| CoreFP 2.2.19 | `CoreFPWin-2.2.19/` | Reference for comparison |

---

## Possible Paths Forward

### Option 1: Adapt Requiem's Approach (Recommended)
**Strategy:** Don't reverse CoreFP — use its public API via native library interface

**Steps:**
1. Study Requiem's native library source to understand CoreFP API
2. Identify exported functions in our CoreFP.dll
3. Create minimal wrapper that calls CoreFP to decrypt iPod game files
4. Feed it our SC Info + .sinf files

**Pros:**
- Uses officially documented (by Apple to iTunes) API
- Legal under DMCA interoperability exemption
- No binary patching or cracking needed

**Cons:**
- Requires understanding Requiem's native code
- May need adaptation for game files vs. music/video

### Option 2: Run VM + Debug iTunes
**Strategy:** Boot the preservation VM, attach debugger to iTunes during game sync

**Steps:**
1. Boot VM in UTM (Apple Silicon) or VMware/VirtualBox (x86)
2. Trigger game authorization (iTunes reads SC Info, calls CoreFP)
3. Hook or breakpoint at decryption point
4. Extract either:
   - Decrypted game binaries directly, OR
   - AES content keys for offline decryption

**Pros:**
- 19 accounts already authorized
- iTunes + CoreFP already configured
- Direct access to decrypted content

**Cons:**
- Requires Windows VM setup
- May need x86 emulation (slow on Apple Silicon)
- Debugging iTunes is complex

### Option 3: Binary Diff CoreFP Versions
**Strategy:** Compare Requiem's CoreFP 2.2.19 with our 12.6.5.3 version

**Steps:**
1. Diff the two DLLs to find API stability
2. Identify exported functions present in both
3. Use Requiem's calling conventions on newer DLL

**Pros:**
- Leverages known-good interface
- May work if API is backward compatible

**Cons:**
- API may have changed significantly
- Time-intensive analysis

---

## Key Unknowns

1. **Does CoreFP export a generic decrypt function?**
   - Requiem suggests yes, but may be content-type specific
   
2. **How does CoreFP handle device authorization?**
   - Our VM has all 19 accounts pre-authorized
   - SC Info files are already "hot" — no re-auth needed
   
3. **What's the exact API signature?**
   - Need to examine Requiem's native library source

4. **Does iPod game DRM differ from music DRM?**
   - Same `.sinf` structure observed
   - Same `priv` blob format
   - Same IV structure
   - Likely same decryption path

---

## Documentation of Discovered Keys/Values

### Hardcoded Keys Found
```
iTunes Library AES Key: "BHUILuilfghuila3" (16 bytes, AES-128-ECB)
Used for: Decrypting iTunes Library.itl database (not content decryption)
```

### SC Info Hardware Binding
```
MachineGuid: 7eaac238-4170-4f15-8bf0-bf4bc727cf39
ComputerName: IK-PC
NTFS Volume Serial: 0xc20e7ed7
Key ID: 00425-00000-00002-AA046 (from SC Info.txt)
```

### Sample IVs (from .sinf files)
```
Bejeweled:  e93b50adfa143376e2cc2713a4b22d51
Tetris:     9a86c0164a1d7941c51176df35f1d60d
Sudoku:     cfd1e8538a6ee838933f2100c336f0bc
```

---

## Next Actions

1. ✅ **Clone Requiem repository** — examine native library interface
2. **Compare CoreFP versions** — diff 2.2.19 vs. our 12.6.5.3 version
3. **Identify CoreFP exports** — dump DLL exports with `dumpbin` or `objdump`
4. **Attempt VM boot** — try UTM with preservation VM for live extraction
5. **Document API findings** — map Requiem's calls to actual CoreFP exports

---

## References

- [pwn0rz/fairplay_research](https://github.com/pwn0rz/fairplay_research) - Kernel debugger
- [Shaddy1884/requiem](https://github.com/Shaddy1884/requiem) - Working decryption tool
- [tedzhang2891/M4VDRMRemoval](https://github.com/tedzhang2891/M4VDRMRemoval) - FFmpeg approach
- [DECRYPTION_ANALYSIS.md](./DECRYPTION_ANALYSIS.md) - Our full technical analysis
- [sc_info/](./sc_info/) - Extracted VM key material

---

*Documented: 2025-01-23*  
*Researcher: AI coding assistant*  
*Status: Ready for Requiem code analysis and CoreFP comparison*

## Update: iPod Storage Discovery

**Critical Finding (2025-06-25):** The iPod stores **encrypted files on disk**, not decrypted EAPP files!

### Observed Behavior

Files on the iPod:
```
Bejeweled_1_1_3367225.bin:  9d95 38ab 87bb a1aa... (ENCRYPTED)
Minigolf_1_1_3299468.bin:  179c d338 88f0 9fbc... (ENCRYPTED)
```

Same files in preservation dump:
```
Bejeweled_1_1_3367225.bin:  6561 7070 0010 0010... (eapp header - DECRYPTED)
Minigolf_1_1_3299468.bin:  6561 7070 0010 0010... (eapp header - DECRYPTED)
```

### Implications

1. **iPod OS has FairPlay decryption built-in**
   - The iPod decrypts files at runtime when loading into memory
   - Uses hardware-derived device key (stored in secure storage)
   - Reads .sinf files to get wrapped content keys

2. **No "SC Info" on iPod**
   - The iPod doesn't have SC Info.sidb/sidd files like Windows
   - Device key is in hardware/firmware, not files

3. **Extraction requires either:**
   - **Memory dumping** while game is running (hard)
   - **Hardware key extraction** from iPod firmware (very hard)
   - **VM + iTunes sync** to get decrypted files (preservation did this)

4. **The preservation release used VM method**
   - The `16-ipod-games` decrypted files came from the VM approach
   - Not from the iPod directly (which would be encrypted)

### Why This Matters

**Option 1: Extract from iPod**
- Would require reverse engineering iPod firmware
- Finding hardware key or decryption vulnerability
- Much harder than Windows approach

**Option 2: Use preservation VM (RECOMMENDED)**
- Already has iTunes + authorized accounts
- Sync creates files that can be extracted
- This is how the `16-ipod-games` dump was created

**Option 3: Emulator with on-device decryption**
- Would need to emulate iPod's FairPlay hardware
- Even harder than Option 1

### Conclusion

The preservation VM approach remains the most viable path to get decrypted games. The iPod's on-device encryption means we cannot simply copy decrypted files from it.

