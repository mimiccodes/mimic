#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

// Heat over a 1,024 × 1,024 plate for 400 steps (each inner cell: itself
// weighted 4 and its four neighbours, over 8; the edge stays), starting
// from splitmix64: the sum of the cells and the centre, in thousandths.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    const int64_t n = 1024, steps = 400;
    double *g = malloc(sizeof(double) * n * n), *h = malloc(sizeof(double) * n * n);
    for (int64_t p = 0; p < n * n; p++) g[p] = (double)(mix(p) % 100000) / 1000.0;
    for (int64_t s = 0; s < steps; s++) {
        for (int64_t r = 0; r < n; r++)
            for (int64_t c = 0; c < n; c++) {
                int64_t p = r * n + c;
                if (r == 0 || c == 0 || r == n - 1 || c == n - 1) h[p] = g[p];
                else {
                    double x = g[p];
                    h[p] = (x + x + x + x + g[p - n] + g[p + n] + g[p - 1] + g[p + 1]) * 0.125;
                }
            }
        double *t = g; g = h; h = t;
    }
    int64_t total = 0;
    for (int64_t p = 0; p < n * n; p++) total += (int64_t)(g[p] * 1000.0);
    printf("%lld %lld\n", (long long)total, (long long)(g[n * (n / 2) + n / 2] * 1000.0));
    free(g);
    free(h);
    return 0;
}
