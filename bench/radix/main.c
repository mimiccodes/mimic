// radix: 8,000,000 32-bit keys (from splitmix64) sorted by LSD radix sort,
// four passes of 8 bits (count, running offsets, scatter). Prints the
// smallest, the median and the largest, and a position-weighted checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 8000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t *a = malloc(sizeof(int64_t) * N), *b = malloc(sizeof(int64_t) * N);
    for (int64_t k = 0; k < N; k++) a[k] = mix(k) % 4294967296;
    for (int s = 0; s < 32; s += 8) {
        int64_t pos[256] = {0};
        for (int64_t i = 0; i < N; i++) pos[a[i] >> s & 255]++;
        int64_t run = 0;
        for (int d = 0; d < 256; d++) {
            int64_t c = pos[d];
            pos[d] = run;
            run += c;
        }
        for (int64_t i = 0; i < N; i++) b[pos[a[i] >> s & 255]++] = a[i];
        int64_t *t = a;
        a = b;
        b = t;
    }
    int64_t check = 0;
    for (int64_t i = 0; i < N; i++) check += a[i] * (i % 251 + 1);
    printf("%lld %lld %lld %lld\n", (long long)a[0], (long long)a[N / 2], (long long)a[N - 1], (long long)check);
    free(a);
    free(b);
    return 0;
}
