// rule110: the elementary cellular automaton Rule 110 on a ring of 500,000
// cells (start from splitmix64) for 1,000 generations. Prints the number of
// live cells at the end and the sum of their positions.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 500000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    uint8_t *c = malloc(N), *d = malloc(N);
    for (int64_t i = 0; i < N; i++) c[i] = mix(i) % 3 == 0;
    for (int g = 0; g < 1000; g++) {
        for (int64_t i = 0; i < N; i++) d[i] = 110 >> (c[(i + N - 1) % N] * 4 + c[i] * 2 + c[(i + 1) % N]) & 1;
        uint8_t *t = c;
        c = d;
        d = t;
    }
    int64_t live = 0, pos = 0;
    for (int64_t i = 0; i < N; i++) {
        live += c[i];
        pos += c[i] * i;
    }
    printf("%lld %lld\n", (long long)live, (long long)pos);
    return 0;
}
