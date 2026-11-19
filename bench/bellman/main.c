// bellman: single-source shortest paths from node 0 in a random directed
// graph of 50,000 nodes and 400,000 weighted edges (from splitmix64), by
// Bellman-Ford: every edge relaxed, round after round, until nothing
// changes. Prints the number of nodes reached, the sum of their distances,
// and the largest distance.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 50000
#define M 400000
#define INF ((int64_t)1 << 60)

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    static int64_t src[M], dst[M], w[M], d[N];
    for (int64_t e = 0; e < M; e++) {
        src[e] = mix(3 * e) % N;
        dst[e] = mix(3 * e + 1) % N;
        w[e] = 1 + mix(3 * e + 2) % 1000;
    }
    for (int i = 0; i < N; i++) d[i] = INF;
    d[0] = 0;
    for (int changed = 1; changed;) {
        changed = 0;
        for (int64_t e = 0; e < M; e++)
            if (d[src[e]] < INF && d[src[e]] + w[e] < d[dst[e]]) {
                d[dst[e]] = d[src[e]] + w[e];
                changed = 1;
            }
    }
    int64_t reached = 0, sum = 0, most = 0;
    for (int i = 0; i < N; i++)
        if (d[i] < INF) {
            reached++;
            sum += d[i];
            if (d[i] > most) most = d[i];
        }
    printf("%lld %lld %lld\n", (long long)reached, (long long)sum, (long long)most);
    return 0;
}
