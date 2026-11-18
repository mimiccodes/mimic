#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

// 1,000,000 splitmix64 points (whole numbers up to 100,000) in 16 clusters
// by Lloyd's algorithm, 25 rounds, in whole numbers (a centre moves to the
// mean of its points rounded down; one with none stays): the sum of the
// centres' coordinates, the first cluster's points and the total squared
// distance to the nearest centre.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

enum { N = 1000000, K = 16 };
static int64_t xs[N], ys[N], cx[K], cy[K];

static int nearest(int64_t x, int64_t y, int64_t *best) {
    int at = 0;
    *best = INT64_MAX;
    for (int j = 0; j < K; j++) {
        int64_t dx = cx[j] - x, dy = cy[j] - y, d = dx * dx + dy * dy;
        if (d < *best) { *best = d; at = j; }
    }
    return at;
}

int main(void) {
    for (int64_t i = 0; i < N; i++) { xs[i] = mix(2 * i) % 100001; ys[i] = mix(2 * i + 1) % 100001; }
    for (int j = 0; j < K; j++) { cx[j] = xs[j]; cy[j] = ys[j]; }
    for (int r = 0; r < 25; r++) {
        int64_t sx[K] = {0}, sy[K] = {0}, count[K] = {0}, d;
        for (int64_t i = 0; i < N; i++) {
            int a = nearest(xs[i], ys[i], &d);
            sx[a] += xs[i]; sy[a] += ys[i]; count[a]++;
        }
        for (int j = 0; j < K; j++)
            if (count[j]) { cx[j] = sx[j] / count[j]; cy[j] = sy[j] / count[j]; }
    }
    int64_t first = 0, spread = 0, sum = 0, d;
    for (int64_t i = 0; i < N; i++) {
        first += nearest(xs[i], ys[i], &d) == 0;
        spread += d;
    }
    for (int j = 0; j < K; j++) sum += cx[j] + cy[j];
    printf("%lld %lld %lld\n", (long long)sum, (long long)first, (long long)spread);
    return 0;
}
