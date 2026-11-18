// topo: a topological order of a DAG (the bfs graph, edges from the smaller
// node to the larger, every second edge) by Kahn's algorithm, level by
// level, ascending within a level. Prints the levels and a checksum of the
// order.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int cmp(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

int main(void) {
    const int64_t n = 200000, m = 1000000;
    int *ea = malloc(sizeof(int) * m), *eb = malloc(sizeof(int) * m), *start = calloc(n + 1, sizeof(int)), *indeg = calloc(n, sizeof(int));
    int64_t k = 0;
    for (int64_t e = 0; e < m; e += 2) {
        int x = (int)(mix(2 * e + 1) % n), y = (int)(mix(2 * e + 2) % n);
        if (x == y) continue;
        ea[k] = x < y ? x : y;
        eb[k] = x < y ? y : x;
        start[ea[k] + 1]++;
        indeg[eb[k]]++;
        k++;
    }
    for (int64_t v = 0; v < n; v++) start[v + 1] += start[v];
    int *out = malloc(sizeof(int) * k), *fill = malloc(sizeof(int) * n);
    for (int64_t v = 0; v < n; v++) fill[v] = start[v];
    for (int64_t i = 0; i < k; i++) out[fill[ea[i]]++] = eb[i];
    int *level = malloc(sizeof(int) * n), *next = malloc(sizeof(int) * n);
    int nl = 0, nn;
    for (int64_t v = 0; v < n; v++) if (indeg[v] == 0) level[nl++] = (int)v;
    int64_t levels = 0, pos = 0, check = 0;
    while (nl > 0) {
        levels++;
        nn = 0;
        for (int i = 0; i < nl; i++) {
            int v = level[i];
            check = (check + pos * v) % 1000000007;
            pos++;
            for (int j = start[v]; j < start[v + 1]; j++)
                if (--indeg[out[j]] == 0) next[nn++] = out[j];
        }
        qsort(next, nn, sizeof(int), cmp);
        int *t = level;
        level = next;
        next = t;
        nl = nn;
    }
    printf("%lld %lld\n", (long long)levels, (long long)check);
    return 0;
}
