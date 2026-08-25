// Simple CoreFP.dll test
// Build: x86_64-w64-mingw32-gcc -o test_simple.exe test_simple.c
// Run: wine test_simple.exe

#include <windows.h>
#include <stdio.h>

int main() {
    printf("Loading CoreFP.dll...\n");
    
    HMODULE hCoreFP = LoadLibrary("CoreFP.dll");
    if (!hCoreFP) {
        DWORD err = GetLastError();
        printf("Failed to load: error %lu\n", err);
        
        // Try with full path
        hCoreFP = LoadLibrary("C:\\Program Files\\iTunes\\CoreFP.dll");
        if (!hCoreFP) {
            printf("Also failed with full path: %lu\n", GetLastError());
            return 1;
        }
    }
    
    printf("Loaded successfully!\n");
    
    // List exports
    PIMAGE_DOS_HEADER dos = (PIMAGE_DOS_HEADER)hCoreFP;
    PIMAGE_NT_HEADERS nt = (PIMAGE_NT_HEADERS)((BYTE*)hCoreFP + dos->e_lfanew);
    PIMAGE_EXPORT_DIRECTORY exports = (PIMAGE_EXPORT_DIRECTORY)((BYTE*)hCoreFP + 
        nt->OptionalHeader.DataDirectory[0].VirtualAddress);
    
    DWORD *names = (DWORD*)((BYTE*)hCoreFP + exports->AddressOfNames);
    
    printf("Exports found:\n");
    for (DWORD i = 0; i < exports->NumberOfNames && i < 20; i++) {
        char *name = (char*)((BYTE*)hCoreFP + names[i]);
        printf("  %s\n", name);
    }
    
    // Check specific exports we need
    FARPROC WIn9 = GetProcAddress(hCoreFP, "WIn9UJ86JKdV4dM");
    FARPROC X46O = GetProcAddress(hCoreFP, "X46O5IeS");
    FARPROC YlCJ = GetProcAddress(hCoreFP, "YlCJ3lg");
    
    printf("\nKey functions:\n");
    printf("  WIn9UJ86JKdV4dM: %p\n", WIn9);
    printf("  X46O5IeS: %p\n", X46O);
    printf("  YlCJ3lg: %p\n", YlCJ);
    
    FreeLibrary(hCoreFP);
    return 0;
}
