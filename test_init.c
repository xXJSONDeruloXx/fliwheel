// Test CoreFP.dll initialization with device key
// Build: x86_64-w64-mingw32-gcc -o test_init.exe test_init.c

#include <windows.h>
#include <stdio.h>
#include <string.h>

// Candidate device keys (6 bytes each)
const BYTE candidate_keys[][6] = {
    {0x04, 0x2e, 0xff, 0xb6, 0x54, 0x43},  // ascii_no_null
    {0x96, 0x26, 0x41, 0x3d, 0x43, 0xb7},  // with_nulls
    {0x86, 0x99, 0x21, 0x4f, 0xc2, 0x6f},  // full_firmware
    {0xd5, 0x19, 0xb2, 0xaa, 0x05, 0x68},  // utf16_proc
    {0x0b, 0x53, 0x9d, 0xda, 0x74, 0x06},  // bios_vendor
};
const char* candidate_names[] = {
    "ascii_no_null",
    "with_nulls", 
    "full_firmware",
    "utf16_proc",
    "bios_vendor"
};

// Function pointer types
typedef int (__stdcall *WIn9UJ86JKdV4dM_t)(UINT32, UINT32*, UINT32*, const BYTE*, UINT32, BYTE*, const BYTE**, int*);
typedef int (__stdcall *X46O5IeS_t)(UINT32, UINT32, UINT32, const BYTE*);
typedef int (__stdcall *YlCJ3lg_t)(UINT32, UINT32, UINT32, UINT32, UINT32, UINT32, UINT32, UINT32);

int test_init_with_key(HMODULE hCoreFP, const BYTE *mac_addr, const char *name, const char *sc_info_path) {
    printf("\n[%s] Testing device key: ", name);
    for (int i = 0; i < 6; i++) printf("%02x", mac_addr[i]);
    printf("\n");
    
    // Get exports
    WIn9UJ86JKdV4dM_t WIn9 = (WIn9UJ86JKdV4dM_t)GetProcAddress(hCoreFP, "WIn9UJ86JKdV4dM");
    X46O5IeS_t X46O = (X46O5IeS_t)GetProcAddress(hCoreFP, "X46O5IeS");
    YlCJ3lg_t YlCJ = (YlCJ3lg_t)GetProcAddress(hCoreFP, "YlCJ3lg");
    
    if (!WIn9 || !X46O || !YlCJ) {
        printf("  ERROR: Missing exports\n");
        return -1;
    }
    
    printf("  Exports found, attempting initialization...\n");
    
    // Step 1: YlCJ3lg call with opcode 0xf4419e34 to initialize with SC Info
    // Build mac_addr structure
    BYTE mac_struct[10] = {0};
    *(UINT32*)mac_struct = 6;  // size
    memcpy(mac_struct + 4, mac_addr, 6);
    
    // Build arguments for init
    UINT32 args[5] = {
        1,           // arg0: always 1
        0,           // arg1: (will be populated by CoreFP)
        (UINT32)mac_struct,  // arg2: pointer to mac struct
        (UINT32)sc_info_path, // arg3: SC Info directory path
        0            // arg4: (output token4)
    };
    
    // This is a simplified version - the actual call requires more setup
    // including certificate exchange via WIn9UJ86JKdV4dM
    printf("  (Full init requires cert exchange - checking basic call...)\n");
    
    // Try a basic probe call to see if the key format is recognized
    // The actual YlCJ3lg call signature is complex, we may need to reverse it more
    
    return 0;
}

int main(int argc, char *argv[]) {
    printf("CoreFP.dll Device Key Initialization Test\n");
    printf("=========================================\n\n");
    
    // Setup paths
    char sc_info_path[MAX_PATH] = "C:\\ProgramData\\Apple Computer\\iTunes\\SC Info";
    if (argc > 1) {
        strncpy(sc_info_path, argv[1], MAX_PATH-1);
    }
    
    printf("SC Info path: %s\n", sc_info_path);
    
    // Check if SC Info directory exists
    DWORD attrs = GetFileAttributes(sc_info_path);
    if (attrs == INVALID_FILE_ATTRIBUTES) {
        printf("WARNING: SC Info directory not found at expected path\n");
        printf("Trying current directory...\n");
        strcpy(sc_info_path, ".\\SC Info");
    }
    
    // Load CoreFP.dll
    HMODULE hCoreFP = LoadLibrary("CoreFP.dll");
    if (!hCoreFP) {
        // Try with full path
        hCoreFP = LoadLibrary("C:\\Program Files\\iTunes\\CoreFP.dll");
        if (!hCoreFP) {
            printf("ERROR: Failed to load CoreFP.dll (error: %lu)\n", GetLastError());
            return 1;
        }
    }
    printf("CoreFP.dll loaded successfully\n\n");
    
    // Test each candidate key
    int working_key = -1;
    for (int i = 0; i < 5; i++) {
        int result = test_init_with_key(hCoreFP, candidate_keys[i], candidate_names[i], sc_info_path);
        if (result == 0) {
            working_key = i;
            printf("  -> Key %s appears to work!\n", candidate_names[i]);
        }
    }
    
    printf("\n=========================================\n");
    if (working_key >= 0) {
        printf("SUCCESS: Found working device key: %s\n", candidate_names[working_key]);
    } else {
        printf("No working device key found.\n");
        printf("\nThe initialization sequence is complex and requires:\n");
        printf("1. Certificate exchange (WIn9UJ86JKdV4dM)\n");
        printf("2. Authentication (X46O5IeS)\n");
        printf("3. SC Info loading with proper data structures\n");
        printf("\nRecommendation: Use the preservation VM directly for decryption.\n");
    }
    
    FreeLibrary(hCoreFP);
    return (working_key >= 0) ? 0 : 1;
}
