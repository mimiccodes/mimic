// mst: Kruskal's minimum spanning forest of a random graph of 200,000
// nodes and 1,000,000 edges (from splitmix64): edges sorted by weight then
// number, union-find by size. Prints the total weight of the forest, its
// number of edges, and the sum of the numbers of the edges taken.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 200000
#define M 1000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

static int64_t parent[N], size[N];

static int64_t find(int64_t x) {
    while (parent[x] != x) x = parent[x];
    return x;
}

int main(void) {
    int64_t *keys = malloc(sizeof(int64_t) * M);
    for (int64_t e = 0; e < M; e++) keys[e] = (mix(3 * e + 2) % 1000000) << 20 | e;
    qsort(keys, M, sizeof(int64_t), cmp);
    for (int64_t i = 0; i < N; i++) {
        parent[i] = i;
        size[i] = 1;
    }
    int64_t total = 0, count = 0, ids = 0;
    for (int64_t k = 0; k < M; k++) {
        int64_t w = keys[k] >> 20, e = keys[k] & ((1 << 20) - 1);
        int64_t a = find(mix(3 * e) % N), b = find(mix(3 * e + 1) % N);
        if (a == b) continue;
        if (size[a] < size[b]) {
            int64_t t = a;
            a = b;
            b = t;
        }
        parent[b] = a;
        size[a] += size[b];
        total += w;
        count++;
        ids += e;
    }
    printf("%lld %lld %lld\n", (long long)total, (long long)count, (long long)ids);
    return 0;
}
