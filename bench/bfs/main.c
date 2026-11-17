// bfs: breadth-first distances from node 0 in a random graph of 200,000
// nodes and 1,000,000 directed edges (both ends from splitmix64 of the
// edge number), with a queue over adjacency lists (CSR). Prints the nodes
// reached, the sum of their distances and the largest.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    const int64_t n = 200000, m = 1000000;
    int *src = malloc(sizeof(int) * m), *dst = malloc(sizeof(int) * m), *start = calloc(n + 1, sizeof(int));
    for (int64_t e = 0; e < m; e++) {
        src[e] = (int)(mix(2 * e + 1) % n);
        dst[e] = (int)(mix(2 * e + 2) % n);
        start[src[e] + 1]++;
    }
    for (int64_t v = 0; v < n; v++) start[v + 1] += start[v];
    int *adj = malloc(sizeof(int) * m), *fill = malloc(sizeof(int) * n);
    for (int64_t v = 0; v < n; v++) fill[v] = start[v];
    for (int64_t e = 0; e < m; e++) adj[fill[src[e]]++] = dst[e];
    int *dist = malloc(sizeof(int) * n), *queue = malloc(sizeof(int) * n);
    for (int64_t v = 0; v < n; v++) dist[v] = -1;
    int head = 0, tail = 0;
    dist[0] = 0;
    queue[tail++] = 0;
    while (head < tail) {
        int u = queue[head++];
        for (int k = start[u]; k < start[u + 1]; k++)
            if (dist[adj[k]] < 0) {
                dist[adj[k]] = dist[u] + 1;
                queue[tail++] = adj[k];
            }
    }
    int64_t reached = 0, sum = 0, far = 0;
    for (int64_t v = 0; v < n; v++)
        if (dist[v] >= 0) {
            reached++;
            sum += dist[v];
            if (dist[v] > far) far = dist[v];
        }
    printf("%lld %lld %lld\n", (long long)reached, (long long)sum, (long long)far);
    return 0;
}
