// siphash: SipHash-2-4 (key bytes 0-15) of 2,000,000 messages of 16 bytes
// (two 64-bit words from splitmix64 each), as a hash table hashes its keys.
// Prints the exclusive or of the hashes and their sum modulo 1,000,000,007.
#include <stdint.h>
#include <stdio.h>

static uint64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

static uint64_t rotl(uint64_t x, int r) { return x << r | x >> (64 - r); }

#define ROUND                                                                                                                                        \
    do {                                                                                                                                             \
        v0 += v1; v1 = rotl(v1, 13) ^ v0; v0 = rotl(v0, 32);                                                                                         \
        v2 += v3; v3 = rotl(v3, 16) ^ v2;                                                                                                            \
        v0 += v3; v3 = rotl(v3, 21) ^ v0;                                                                                                            \
        v2 += v1; v1 = rotl(v1, 17) ^ v2; v2 = rotl(v2, 32);                                                                                         \
    } while (0)

// two words of message, 16 bytes in all
static uint64_t siphash(uint64_t k0, uint64_t k1, uint64_t m0, uint64_t m1) {
    uint64_t v0 = k0 ^ 0x736F6D6570736575ull, v1 = k1 ^ 0x646F72616E646F6Dull, v2 = k0 ^ 0x6C7967656E657261ull, v3 = k1 ^ 0x7465646279746573ull;
    uint64_t ms[3] = {m0, m1, (uint64_t)16 << 56};
    for (int i = 0; i < 3; i++) {
        v3 ^= ms[i];
        ROUND;
        ROUND;
        v0 ^= ms[i];
    }
    v2 ^= 0xFF;
    ROUND;
    ROUND;
    ROUND;
    ROUND;
    return v0 ^ v1 ^ v2 ^ v3;
}

int main(void) {
    uint64_t k0 = 0x0706050403020100ull, k1 = 0x0F0E0D0C0B0A0908ull, x = 0, s = 0;
    for (int64_t j = 0; j < 2000000; j++) {
        uint64_t h = siphash(k0, k1, mix(2 * j), mix(2 * j + 1));
        x ^= h;
        s = (s + h % 1000000007) % 1000000007;
    }
    printf("%016llx %llu\n", (unsigned long long)x, (unsigned long long)s);
    return 0;
}
