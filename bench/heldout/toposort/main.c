// Topological order of a generated DAG (every edge goes from a smaller to a
// larger node) by Kahn's algorithm: a first-in first-out queue, starting
// from the sources in increasing order. Prints a checksum of the order.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define NODES 200000
#define EDGES 600000

int main(void) {
    int64_t *from = malloc(EDGES * sizeof *from), *to = malloc(EDGES * sizeof *to);
    int64_t x = 42;
    for (int e = 0; e < EDGES; e++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t u = x % (NODES - 1);
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t v = u + 1 + x % (NODES - 1 - u);
        from[e] = u;
        to[e] = v;
    }
    // adjacency lists in edge order
    int64_t *start = calloc(NODES + 1, sizeof *start), *indeg = calloc(NODES, sizeof *indeg);
    for (int e = 0; e < EDGES; e++) {
        start[from[e] + 1]++;
        indeg[to[e]]++;
    }
    for (int i = 0; i < NODES; i++) start[i + 1] += start[i];
    int64_t *fill = malloc(NODES * sizeof *fill), *adj = malloc(EDGES * sizeof *adj);
    for (int i = 0; i < NODES; i++) fill[i] = start[i];
    for (int e = 0; e < EDGES; e++) adj[fill[from[e]]++] = to[e];

    int64_t *queue = malloc(NODES * sizeof *queue);
    int head = 0, tail = 0;
    for (int i = 0; i < NODES; i++)
        if (indeg[i] == 0) queue[tail++] = i;
    while (head < tail) {
        int64_t u = queue[head++];
        for (int64_t k = start[u]; k < start[u + 1]; k++)
            if (--indeg[adj[k]] == 0) queue[tail++] = adj[k];
    }
    int64_t sum = 0;
    for (int i = 0; i < tail; i++) sum += queue[i] * (i % 1000 + 1);
    printf("%d\n", tail);
    printf("%lld\n", (long long)sum);
    return 0;
}
