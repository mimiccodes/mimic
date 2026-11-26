// Dijkstra's shortest paths from node 0 on a generated directed graph held
// as adjacency lists (a chain i -> i+1 keeps it connected). Each step picks
// the closest unfinished node by scanning. Prints the sum of the distances.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define NODES 8000
#define EDGES 40000

typedef struct {
    int64_t to, weight;
} Edge;

typedef struct {
    Edge *items;
    int len, cap;
} EdgeList;

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static void add_edge(EdgeList *adj, int64_t from, int64_t to, int64_t w) {
    EdgeList *l = &adj[from];
    if (l->len == l->cap) {
        l->cap = l->cap ? l->cap * 2 : 4;
        l->items = realloc(l->items, l->cap * sizeof *l->items);
    }
    l->items[l->len].to = to;
    l->items[l->len].weight = w;
    l->len++;
}

int main(void) {
    EdgeList *adj = calloc(NODES, sizeof *adj);
    for (int i = 0; i + 1 < NODES; i++) add_edge(adj, i, i + 1, next() % 100 + 1);
    for (int e = NODES - 1; e < EDGES; e++) {
        int64_t u = next() % NODES;
        int64_t v = next() % NODES;
        int64_t w = next() % 100 + 1;
        add_edge(adj, u, v, w);
    }
    const int64_t INF = INT64_MAX;
    int64_t *dist = malloc(NODES * sizeof *dist);
    char *done = calloc(NODES, 1);
    for (int i = 0; i < NODES; i++) dist[i] = INF;
    dist[0] = 0;
    for (;;) {
        int u = -1;
        int64_t best = INF;
        for (int i = 0; i < NODES; i++)
            if (!done[i] && dist[i] < best) {
                best = dist[i];
                u = i;
            }
        if (u < 0) break;
        done[u] = 1;
        for (int k = 0; k < adj[u].len; k++) {
            Edge e = adj[u].items[k];
            if (best + e.weight < dist[e.to]) dist[e.to] = best + e.weight;
        }
    }
    int64_t sum = 0;
    for (int i = 0; i < NODES; i++) sum += dist[i];
    printf("%lld\n", (long long)sum);
    return 0;
}
