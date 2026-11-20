// bitset: 100 sets over the numbers below 2^20, each 16,384 words of 64
// bits (from splitmix64), and for every pair the sizes of their
// intersection and union (hardware population count). Prints the sum of
// the intersection sizes, the sum of the union sizes, and the pair with the
// largest intersection as i * 100 + j.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define S 100
#define W 16384

static uint64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    uint64_t *ws = malloc(sizeof(uint64_t) * S * W);
    for (int64_t k = 0; k < (int64_t)S * W; k++) ws[k] = mix(3 * k) & mix(3 * k + 1) & mix(3 * k + 2);
    int64_t inter = 0, uni = 0, best = -1, pair = 0;
    for (int i = 0; i < S; i++)
        for (int j = i + 1; j < S; j++) {
            int64_t a = 0, u = 0;
            for (int w = 0; w < W; w++) {
                uint64_t x = ws[(int64_t)i * W + w], y = ws[(int64_t)j * W + w];
                a += __builtin_popcountll(x & y);
                u += __builtin_popcountll(x | y);
            }
            inter += a;
            uni += u;
            if (a > best) {
                best = a;
                pair = i * 100 + j;
            }
        }
    printf("%lld %lld %lld\n", (long long)inter, (long long)uni, (long long)pair);
    return 0;
}
