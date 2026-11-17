// components: the connected components of the bfs graph at half the size,
// undirected, every third edge left out, with union-find (each root
// joined under the smaller; no path compression).
// Prints the components and the size of the largest.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int *parent;
static int find(int x) {
    while (parent[x] != x) x = parent[x];
    return x;
}

int main(void) {
    const int64_t n = 100000, m = 500000;
    parent = malloc(sizeof(int) * n);
    int *size = calloc(n, sizeof(int));
    for (int64_t v = 0; v < n; v++) parent[v] = (int)v;
    for (int64_t e = 0; e < m; e++) {
        if (e % 3 == 0) continue;
        int a = find((int)(mix(2 * e + 1) % n)), b = find((int)(mix(2 * e + 2) % n));
        if (a != b) parent[a > b ? a : b] = a < b ? a : b;
    }
    int64_t comps = 0, largest = 0;
    for (int64_t v = 0; v < n; v++) size[find((int)v)]++;
    for (int64_t v = 0; v < n; v++) {
        if (size[v] > 0) comps++;
        if (size[v] > largest) largest = size[v];
    }
    printf("%lld %lld\n", (long long)comps, (long long)largest);
    return 0;
}
