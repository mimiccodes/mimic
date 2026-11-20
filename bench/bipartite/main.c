// bipartite: a random undirected graph of 300,000 nodes and 200,000 edges
// (from splitmix64), each component two-coloured by breadth-first search
// from its smallest node (adjacency lists in edge order). Prints the
// number of components, how many are bipartite, and the size of the
// largest.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 300000
#define M 200000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    static int64_t deg[N + 1], start[N + 1], fill[N], adj[2 * M], colour[N], q[N];
    for (int64_t h = 0; h < 2 * M; h++) deg[mix(h) % N]++;
    for (int64_t v = 0; v < N; v++) start[v + 1] = start[v] + deg[v];
    for (int64_t h = 0; h < 2 * M; h++) {
        int64_t v = mix(h) % N;
        adj[start[v] + fill[v]++] = mix(2 * (h / 2) + 1 - h % 2) % N;
    }
    for (int64_t v = 0; v < N; v++) colour[v] = -1;
    int64_t comps = 0, bip = 0, largest = 0;
    for (int64_t s = 0; s < N; s++) {
        if (colour[s] >= 0) continue;
        comps++;
        colour[s] = 0;
        int64_t head = 0, tail = 0, ok = 1;
        q[tail++] = s;
        while (head < tail) {
            int64_t u = q[head++];
            for (int64_t i = start[u]; i < start[u + 1]; i++) {
                int64_t v = adj[i];
                if (colour[v] < 0) {
                    colour[v] = 1 - colour[u];
                    q[tail++] = v;
                } else if (colour[v] == colour[u]) ok = 0;
            }
        }
        bip += ok;
        if (tail > largest) largest = tail;
    }
    printf("%lld %lld %lld\n", (long long)comps, (long long)bip, (long long)largest);
    return 0;
}
