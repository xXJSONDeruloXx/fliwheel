// Test program to try loading CoreFP.dll with device key candidates
// Build: x86_64-w64-mingw32-gcc -o test_corefp.exe test_corefp.c -ladvapi32 -lcrypt32

#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <wincrypt.h>

#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "crypt32.lib")

// Candidate device keys from docs/DEVICE_KEY_DERIVATION.md
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

// Simplified MD5 implementation
#define F(x, y, z) (((x) & (y)) | ((~x) & (z)))
#define G(x, y, z) (((x) & (z)) | ((y) & (~z)))
#define H(x, y, z) ((x) ^ (y) ^ (z))
#define I(x, y, z) ((y) ^ ((x) | (~z)))
#define ROTATE_LEFT(x, n) (((x) << (n)) | ((x) >> (32-(n))))

void md5_transform(UINT state[4], const BYTE block[64]) {
    UINT a = state[0], b = state[1], c = state[2], d = state[3], x[16];
    
    // Decode block into 16 UINTs
    for (int i = 0; i < 16; i++) {
        x[i] = block[i*4] | (block[i*4+1] << 8) | (block[i*4+2] << 16) | (block[i*4+3] << 24);
    }
    
    // Round 1
    #define FF(a, b, c, d, x, s, ac) { \
        (a) += F((b), (c), (d)) + (x) + (UINT)(ac); \
        (a) = ROTATE_LEFT((a), (s)); \
        (a) += (b); \
    }
    FF(a, b, c, d, x[0], 7, 0xd76aa478);
    FF(d, a, b, c, x[1], 12, 0xe8c7b756);
    FF(c, d, a, b, x[2], 17, 0x242070db);
    FF(b, c, d, a, x[3], 22, 0xc1bdceee);
    FF(a, b, c, d, x[4], 7, 0xf57c0faf);
    FF(d, a, b, c, x[5], 12, 0x4787c62a);
    FF(c, d, a, b, x[6], 17, 0xa8304613);
    FF(b, c, d, a, x[7], 22, 0xfd469501);
    FF(a, b, c, d, x[8], 7, 0x698098d8);
    FF(d, a, b, c, x[9], 12, 0x8b44f7af);
    FF(c, d, a, b, x[10], 17, 0xffff5bb1);
    FF(b, c, d, a, x[11], 22, 0x895cd7be);
    FF(a, b, c, d, x[12], 7, 0x6b901122);
    FF(d, a, b, c, x[13], 12, 0xfd987193);
    FF(c, d, a, b, x[14], 17, 0xa679438e);
    FF(b, c, d, a, x[15], 22, 0x49b40821);
    #undef FF
    
    // Round 2
    #define GG(a, b, c, d, x, s, ac) { \
        (a) += G((b), (c), (d)) + (x) + (UINT)(ac); \
        (a) = ROTATE_LEFT((a), (s)); \
        (a) += (b); \
    }
    GG(a, b, c, d, x[1], 5, 0xf61e2562);
    GG(d, a, b, c, x[6], 9, 0xc040b340);
    GG(c, d, a, b, x[11], 14, 0x265e5a51);
    GG(b, c, d, a, x[0], 20, 0xe9b6c7aa);
    GG(a, b, c, d, x[5], 5, 0xd62f105d);
    GG(d, a, b, c, x[10], 9, 0x02441453);
    GG(c, d, a, b, x[15], 14, 0xd8a1e681);
    GG(b, c, d, a, x[4], 20, 0xe7d3fbc8);
    GG(a, b, c, d, x[9], 5, 0x21e1cde6);
    GG(d, a, b, c, x[14], 9, 0xc33707d6);
    GG(c, d, a, b, x[3], 14, 0xf4d50d87);
    GG(b, c, d, a, x[8], 20, 0x455a14ed);
    GG(a, b, c, d, x[13], 5, 0xa9e3e905);
    GG(d, a, b, c, x[2], 9, 0xfcefa3f8);
    GG(c, d, a, b, x[7], 14, 0x676f02d9);
    GG(b, c, d, a, x[12], 20, 0x8d2a4c8a);
    #undef GG
    
    // Round 3
    #define HH(a, b, c, d, x, s, ac) { \
        (a) += H((b), (c), (d)) + (x) + (UINT)(ac); \
        (a) = ROTATE_LEFT((a), (s)); \
        (a) += (b); \
    }
    HH(a, b, c, d, x[5], 4, 0xfffa3942);
    HH(d, a, b, c, x[8], 11, 0x8771f681);
    HH(c, d, a, b, x[11], 16, 0x6d9d6122);
    HH(b, c, d, a, x[14], 23, 0xfde5380c);
    HH(a, b, c, d, x[1], 4, 0xa4beea44);
    HH(d, a, b, c, x[4], 11, 0x4bdecfa9);
    HH(c, d, a, b, x[7], 16, 0xf6bb4b60);
    HH(b, c, d, a, x[10], 23, 0xbebfbc70);
    HH(a, b, c, d, x[13], 4, 0x289b7ec6);
    HH(d, a, b, c, x[0], 11, 0xeaa127fa);
    HH(c, d, a, b, x[3], 16, 0xd4ef3085);
    HH(b, c, d, a, x[6], 23, 0x04881d05);
    HH(a, b, c, d, x[9], 4, 0xd9d4d039);
    HH(d, a, b, c, x[12], 11, 0xe6db99e5);
    HH(c, d, a, b, x[15], 16, 0x1fa27cf8);
    HH(b, c, d, a, x[2], 23, 0xc4ac5665);
    #undef HH
    
    // Round 4
    #define II(a, b, c, d, x, s, ac) { \
        (a) += I((b), (c), (d)) + (x) + (UINT)(ac); \
        (a) = ROTATE_LEFT((a), (s)); \
        (a) += (b); \
    }
    II(a, b, c, d, x[0], 6, 0xf4292244);
    II(d, a, b, c, x[7], 10, 0x432aff97);
    II(c, d, a, b, x[14], 15, 0xab9423a7);
    II(b, c, d, a, x[5], 21, 0xfc93a039);
    II(a, b, c, d, x[12], 6, 0x655b59c3);
    II(d, a, b, c, x[3], 10, 0x8f0ccc92);
    II(c, d, a, b, x[10], 15, 0xffeff47d);
    II(b, c, d, a, x[1], 21, 0x85845dd1);
    II(a, b, c, d, x[8], 6, 0x6fa87e4f);
    II(d, a, b, c, x[15], 10, 0xfe2ce6e0);
    II(c, d, a, b, x[6], 15, 0xa3014314);
    II(b, c, d, a, x[13], 21, 0x4e0811a1);
    II(a, b, c, d, x[4], 6, 0xf7537e82);
    II(d, a, b, c, x[11], 10, 0xbd3af235);
    II(c, d, a, b, x[2], 15, 0x2ad7d2bb);
    II(b, c, d, a, x[9], 21, 0xeb86d391);
    #undef II
    
    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
}

void md5_init(UINT state[4]) {
    state[0] = 0x67452301;
    state[1] = 0xefcdab89;
    state[2] = 0x98badcfe;
    state[3] = 0x10325476;
}

void md5_update(UINT state[4], BYTE buffer[64], UINT *buflen, UINT *bitlen, const BYTE *input, UINT len) {
    for (UINT i = 0; i < len; i++) {
        buffer[*buflen] = input[i];
        (*buflen)++;
        if (*buflen == 64) {
            md5_transform(state, buffer);
            *buflen = 0;
            *bitlen += 512;
        }
    }
}

void md5_final(BYTE digest[16], UINT state[4], BYTE buffer[64], UINT buflen, UINT bitlen) {
    // Padding
    UINT padlen = (buflen < 56) ? (56 - buflen) : (120 - buflen);
    BYTE padding[128];
    memset(padding, 0, padlen);
    padding[0] = 0x80;
    
    // Append length
    UINT bits = bitlen + buflen * 8;
    padding[padlen] = bits & 0xFF;
    padding[padlen + 1] = (bits >> 8) & 0xFF;
    padding[padlen + 2] = (bits >> 16) & 0xFF;
    padding[padlen + 3] = (bits >> 24) & 0xFF;
    padding[padlen + 4] = 0;
    padding[padlen + 5] = 0;
    padding[padlen + 6] = 0;
    padding[padlen + 7] = 0;
    
    // Copy padding to buffer
    for (UINT i = 0; i < padlen + 8; i++) {
        buffer[buflen++] = padding[i];
        if (buflen == 64) {
            md5_transform(state, buffer);
            buflen = 0;
        }
    }
    
    // Output
    for (int i = 0; i < 4; i++) {
        digest[i*4] = state[i] & 0xFF;
        digest[i*4+1] = (state[i] >> 8) & 0xFF;
        digest[i*4+2] = (state[i] >> 16) & 0xFF;
        digest[i*4+3] = (state[i] >> 24) & 0xFF;
    }
}

void simple_md5(const BYTE *data, UINT len, BYTE out[16]) {
    UINT state[4];
    BYTE buffer[64];
    UINT buflen = 0;
    UINT bitlen = 0;
    
    md5_init(state);
    md5_update(state, buffer, &buflen, &bitlen, data, len);
    md5_final(out, state, buffer, buflen, bitlen);
}

// Test loading CoreFP.dll with a device key
int test_device_key(const BYTE *key, const char *name) {
    printf("Testing device key: %s (", name);
    for (int i = 0; i < 6; i++) {
        printf("%02x", key[i]);
    }
    printf(")\n");
    
    HMODULE hCoreFP = LoadLibrary("CoreFP.dll");
    if (!hCoreFP) {
        printf("  Failed to load CoreFP.dll (error: %lu)\n", GetLastError());
        return -1;
    }
    printf("  Loaded CoreFP.dll\n");
    
    // Get required exports
    FARPROC WIn9UJ86JKdV4dM = GetProcAddress(hCoreFP, "WIn9UJ86JKdV4dV4dM");
    FARPROC X46O5IeS = GetProcAddress(hCoreFP, "X46O5IeS");
    FARPROC YlCJ3lg = GetProcAddress(hCoreFP, "YlCJ3lg");
    
    printf("  WIn9UJ86JKdV4dM: %p\n", WIn9UJ86JKdV4dM);
    printf("  X46O5IeS: %p\n", X46O5IeS);
    printf("  YlCJ3lg: %p\n", YlCJ3lg);
    
    if (!WIn9UJ86JKdV4dM || !X46O5IeS || !YlCJ3lg) {
        printf("  Missing required exports\n");
        FreeLibrary(hCoreFP);
        return -2;
    }
    
    // Note: Full initialization requires:
    // 1. Creating SC Info directory structure in Wine
    // 2. Proper certificate exchange
    // 3. Multiple API calls
    
    printf("  All exports found (full init requires SC Info setup)\n");
    
    FreeLibrary(hCoreFP);
    return 0;
}

int main(int argc, char *argv[]) {
    printf("CoreFP.dll Device Key Test\n");
    printf("==========================\n\n");
    
    // Check command line
    const char *dll_path = (argc > 1) ? argv[1] : "CoreFP.dll";
    printf("DLL path: %s\n\n", dll_path);
    
    // If path is relative, try to find it
    if (!strchr(dll_path, '\\') && !strchr(dll_path, '/')) {
        // Try loading from current directory first
        printf("Looking for %s in current directory...\n", dll_path);
    }
    
    // Test all candidates
    int found_working = -1;
    for (int i = 0; i < 5; i++) {
        printf("\n[%d/5] ", i + 1);
        int result = test_device_key(candidate_keys[i], candidate_names[i]);
        if (result == 0) {
            found_working = i;
        }
    }
    
    printf("\n==========================\n");
    if (found_working >= 0) {
        printf("SUCCESS: Device key '%s' works!\n", candidate_names[found_working]);
    } else {
        printf("No working device key found.\n");
        printf("This may require:\n");
        printf("  1. Full Windows environment (not Wine)\n");
        printf("  2. SC Info files in correct location\n");
        printf("  3. Proper registry structure\n");
    }
    
    return (found_working >= 0) ? 0 : 1;
}
