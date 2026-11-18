#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

// Conway's Game of Life on a 1,024 × 1,024 board that wraps around, 150
// generations from a splitmix64 start (a third alive): the live cells,
// and those of the first row.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    const int64_t n = 1024, steps = 150;
    uint8_t *g = malloc(n * n), *h = malloc(n * n);
    for (int64_t p = 0; p < n * n; p++) g[p] = mix(p) % 3 == 0;
    for (int64_t s = 0; s < steps; s++) {
        for (int64_t r = 0; r < n; r++) {
            int64_t up = (r + n - 1) % n * n, row = r * n, down = (r + 1) % n * n;
            for (int64_t c = 0; c < n; c++) {
                int64_t l = (c + n - 1) % n, rt = (c + 1) % n;
                int around = g[up + l] + g[up + c] + g[up + rt] + g[row + l] + g[row + rt] + g[down + l] + g[down + c] + g[down + rt];
                h[row + c] = around == 3 || (around == 2 && g[row + c]);
            }
        }
        uint8_t *t = g; g = h; h = t;
    }
    int64_t live = 0, first = 0;
    for (int64_t p = 0; p < n * n; p++) live += g[p];
    for (int64_t c = 0; c < n; c++) first += g[c];
    printf("%lld %lld\n", (long long)live, (long long)first);
    free(g);
    free(h);
    return 0;
}
