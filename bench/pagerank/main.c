// pagerank: 20 iterations of PageRank (damping 0.85; rank from nodes
// without outgoing edges is not passed on) on the bfs graph at half the
// size, each node summing the edges into it in order of their source.
// Prints the sum of the ranks and the largest rank times n, to 6 decimals.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t *key;
static int cmp(const void *a, const void *b) {
    int64_t x = key[*(const int *)a], y = key[*(const int *)b];
    return x < y ? -1 : x > y;
}

int main(void) {
    const int64_t n = 100000, m = 500000;
    int *src = malloc(sizeof(int) * m), *dst = malloc(sizeof(int) * m), *order = malloc(sizeof(int) * m);
    int *start = calloc(n + 1, sizeof(int)), *outdeg = calloc(n, sizeof(int));
    key = malloc(sizeof(int64_t) * m);
    for (int64_t e = 0; e < m; e++) {
        src[e] = (int)(mix(2 * e + 1) % n);
        dst[e] = (int)(mix(2 * e + 2) % n);
        key[e] = (int64_t)dst[e] * n + src[e];
        order[e] = (int)e;
        outdeg[src[e]]++;
        start[dst[e] + 1]++;
    }
    qsort(order, m, sizeof(int), cmp);
    for (int64_t v = 0; v < n; v++) start[v + 1] += start[v];
    int *from = malloc(sizeof(int) * m);
    for (int64_t i = 0; i < m; i++) from[i] = src[order[i]];
    double *pr = malloc(sizeof(double) * n), *share = malloc(sizeof(double) * n);
    for (int64_t v = 0; v < n; v++) pr[v] = 1.0 / n;
    for (int it = 0; it < 20; it++) {
        for (int64_t u = 0; u < n; u++) share[u] = outdeg[u] ? pr[u] / outdeg[u] : 0.0;
        for (int64_t v = 0; v < n; v++) {
            double s = 0;
            for (int j = start[v]; j < start[v + 1]; j++) s += share[from[j]];
            pr[v] = 0.15 / n + 0.85 * s;
        }
    }
    double total = 0, top = 0;
    for (int64_t v = 0; v < n; v++) {
        total += pr[v];
        if (pr[v] > top) top = pr[v];
    }
    printf("%.6f %.6f\n", total, top * n);
    return 0;
}
