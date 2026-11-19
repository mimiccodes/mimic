// tsp: the shortest round trip through 20 cities (from splitmix64,
// Manhattan distances) by the Held-Karp dynamic programme over the sets of
// cities in increasing order. Prints the length of the shortest tour and
// the sum of dp over the full set.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 20
#define M (N - 1)
#define INF ((int64_t)1 << 50)

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t xs[N], ys[N], d[N][N];
    for (int c = 0; c < N; c++) {
        xs[c] = mix(2 * c) % 1000;
        ys[c] = mix(2 * c + 1) % 1000;
    }
    for (int a = 0; a < N; a++)
        for (int b = 0; b < N; b++) d[a][b] = llabs(xs[a] - xs[b]) + llabs(ys[a] - ys[b]);
    int64_t *dp = malloc(sizeof(int64_t) * ((size_t)1 << M) * M);
    for (int64_t e = 0; e < ((int64_t)1 << M) * M; e++) dp[e] = INF;
    for (int j = 0; j < M; j++) dp[((int64_t)1 << j) * M + j] = d[0][j + 1];
    for (int64_t s = 1; s < (1 << M); s++)
        for (int j = 0; j < M; j++) {
            if (!(s >> j & 1) || s == (1 << j)) continue;
            int64_t prev = s ^ (1 << j), b = INF;
            for (int k = 0; k < M; k++)
                if (prev >> k & 1) {
                    int64_t c = dp[prev * M + k] + d[k + 1][j + 1];
                    if (c < b) b = c;
                }
            dp[s * M + j] = b;
        }
    int64_t full = (1 << M) - 1, best = INF, sum = 0;
    for (int j = 0; j < M; j++) {
        int64_t t = dp[full * M + j] + d[j + 1][0];
        if (t < best) best = t;
        sum += dp[full * M + j];
    }
    printf("%lld %lld\n", (long long)best, (long long)sum);
    return 0;
}
