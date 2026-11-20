// bitpack: 8,000,000 values of 13 bits (from splitmix64) packed into 64-bit
// words, low bits first, then unpacked again. Prints the number of words,
// the sum of every word modulo 1,000,003, and 1 when unpacking gives the
// values back.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 8000000
#define B 13

static uint64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    int64_t nw = ((int64_t)N * B + 63) / 64;
    uint64_t *vals = malloc(8 * N), *words = calloc(nw + 1, 8);
    for (int64_t k = 0; k < N; k++) vals[k] = mix(k) % (1 << B);
    // a bit buffer: whole words go out as they fill
    uint64_t buf = 0;
    int fill = 0;
    int64_t w = 0;
    for (int64_t k = 0; k < N; k++) {
        buf |= vals[k] << fill;
        if (fill + B >= 64) {
            words[w++] = buf;
            buf = fill ? vals[k] >> (64 - fill) : 0;
            fill = fill + B - 64;
        } else fill += B;
    }
    if (fill) words[w++] = buf;
    uint64_t check = 0;
    for (int64_t i = 0; i < nw; i++) check += words[i] % 1000003;
    int same = 1;
    for (int64_t k = 0; k < N; k++) {
        int64_t p = k * B;
        uint64_t lo = words[p / 64] >> (p % 64);
        uint64_t hi = p % 64 + B > 64 ? words[p / 64 + 1] << (64 - p % 64) : 0;
        same &= ((lo | hi) & ((1 << B) - 1)) == vals[k];
    }
    printf("%lld %llu %d\n", (long long)nw, (unsigned long long)check, same);
    return 0;
}
