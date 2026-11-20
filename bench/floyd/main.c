// floyd: the shortest paths between all pairs of 800 nodes by
// Floyd-Warshall; each node has edges to 8 others (from splitmix64, weights
// 1 to 1,000; of two edges to one node the lighter counts). Prints the sum
// of the distances that exist, how many pairs have no path, and the
// longest shortest path.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define FAR 1000000000000LL

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t n = 800;
    int64_t *d = malloc(sizeof(int64_t) * (size_t)(n * n));
    for (int64_t p = 0; p < n * n; p++) d[p] = FAR;
    for (int64_t i = 0; i < n; i++) {
        for (int64_t e = 0; e < 8; e++) {
            int64_t j = mix(8 * i + e) % n, w = 1 + mix(1000000 + 8 * i + e) % 1000;
            if (w < d[i * n + j]) d[i * n + j] = w;
        }
        d[i * n + i] = 0;
    }
    for (int64_t k = 0; k < n; k++)
        for (int64_t i = 0; i < n; i++) {
            int64_t dik = d[i * n + k];
            for (int64_t j = 0; j < n; j++)
                if (dik + d[k * n + j] < d[i * n + j]) d[i * n + j] = dik + d[k * n + j];
        }
    int64_t sum = 0, none = 0, most = 0;
    for (int64_t p = 0; p < n * n; p++) {
        if (d[p] >= FAR) { none++; continue; }
        sum += d[p];
        if (d[p] > most) most = d[p];
    }
    printf("%lld %lld %lld\n", (long long)sum, (long long)none, (long long)most);
    free(d);
    return 0;
}
