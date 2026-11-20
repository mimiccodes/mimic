// pip: 1,000,000 points tested against a polygon of 500 vertices (a jagged
// square, from splitmix64) by ray casting with the even-odd rule, in exact
// integer arithmetic. Prints how many points are inside and the sum of
// their x coordinates.
#include <stdint.h>
#include <stdio.h>

#define V 500
#define N 1000000
#define R 1000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    static int64_t xs[V], ys[V];
    for (int v = 0; v < V; v++) {
        int s = v / 125, j = v % 125;
        int64_t t = 16000 * j - R, d = mix(500 + v) % 400000;
        int64_t x[4] = {t, R - d, -t, d - R}, y[4] = {d - R, t, R - d, -t};
        xs[v] = x[s];
        ys[v] = y[s];
    }
    int64_t count = 0, sx = 0;
    for (int64_t k = 0; k < N; k++) {
        int64_t x = mix(2 * k) % 2200001 - 1100000, y = mix(2 * k + 1) % 2200001 - 1100000;
        int in = 0;
        for (int i = 0; i < V; i++) {
            int j = (i + 1) % V;
            int64_t d = ys[j] - ys[i], t = (x - xs[i]) * d - (y - ys[i]) * (xs[j] - xs[i]);
            if ((ys[i] > y) != (ys[j] > y) && (d > 0 ? t < 0 : t > 0)) in = !in;
        }
        count += in;
        sx += in ? x : 0;
    }
    printf("%lld %lld\n", (long long)count, (long long)sx);
    return 0;
}
