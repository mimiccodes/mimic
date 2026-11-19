// bloom: a Bloom filter of 8,388,608 one-byte cells and 7 hashes per key
// (double hashing from splitmix64): keys 0 to 999,999 go in, then keys 0 to
// 1,999,999 are looked up. Prints how many lookups say "maybe" and how
// many cells are set.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define CELLS 8388608

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t cell(int64_t x, int64_t j) {
    int64_t h1 = mix(2 * x), h2 = mix(2 * x + 1) | 1;
    return (int64_t)((uint64_t)h1 + (uint64_t)j * (uint64_t)h2) & (CELLS - 1);
}

int main(void) {
    int64_t n = 1000000, maybe = 0, set = 0;
    uint8_t *f = calloc(CELLS, 1);
    for (int64_t x = 0; x < n; x++)
        for (int64_t j = 0; j < 7; j++) f[cell(x, j)] = 1;
    for (int64_t x = 0; x < 2 * n; x++) {
        int all = 1;
        for (int64_t j = 0; j < 7 && all; j++) all = f[cell(x, j)];
        maybe += all;
    }
    for (int64_t c = 0; c < CELLS; c++) set += f[c];
    printf("%lld %lld\n", (long long)maybe, (long long)set);
    free(f);
    return 0;
}
