/* paths: Dijkstra and A* on a 600x600 grid (cells cost 1 to 9 to enter,
 * walls cannot be entered), from the top-left to the bottom-right corner,
 * with a binary heap (lazy deletion). Heap keys are (cost or estimate) * N
 * + node: all different, so every heap pops in the same order. Prints each
 * search's cost and how many nodes it expanded, and the path's length. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 600
#define H 600
#define N (W * H)
#define FAR ((int64_t)1 << 40)

static int64_t cost[N], dist[N], prev_[N], heap[8 * N];
static char closed[N];

static int64_t est(int64_t v, int astar) {
    if (!astar) return 0;
    return (W - 1 - v % W) + (H - 1 - v / W);
}

static void search(int astar, int64_t *out_cost, int64_t *out_expanded) {
    for (int64_t i = 0; i < N; i++) { dist[i] = FAR; prev_[i] = -1; closed[i] = 0; }
    int64_t size = 0, expanded = 0, goal = N - 1;
    dist[0] = 0;
    heap[size++] = est(0, astar) * N + 0;
    while (size > 0) {
        int64_t top = heap[0];
        heap[0] = heap[--size];
        for (int64_t k = 0;;) { /* sift down */
            int64_t l = 2 * k + 1, r = l + 1, m = k;
            if (l < size && heap[l] < heap[m]) m = l;
            if (r < size && heap[r] < heap[m]) m = r;
            if (m == k) break;
            int64_t t = heap[k]; heap[k] = heap[m]; heap[m] = t;
            k = m;
        }
        int64_t u = top % N;
        if (closed[u]) continue;
        closed[u] = 1;
        expanded++;
        if (u == goal) break;
        int64_t x = u % W, y = u / W;
        int64_t nb[4] = {x > 0 ? u - 1 : -1, x < W - 1 ? u + 1 : -1, y > 0 ? u - W : -1, y < H - 1 ? u + W : -1};
        for (int j = 0; j < 4; j++) {
            int64_t v = nb[j];
            if (v < 0 || cost[v] == 0 || closed[v]) continue;
            int64_t d = dist[u] + cost[v];
            if (d < dist[v]) {
                dist[v] = d;
                prev_[v] = u;
                int64_t k = size++; /* sift up */
                heap[k] = (d + est(v, astar)) * N + v;
                while (k > 0 && heap[k] < heap[(k - 1) / 2]) {
                    int64_t p = (k - 1) / 2, t = heap[k]; heap[k] = heap[p]; heap[p] = t;
                    k = p;
                }
            }
        }
    }
    *out_cost = dist[goal];
    *out_expanded = expanded;
}

int main(void) {
    for (int64_t i = 0; i < N; i++) {
        int64_t x = i % W, y = i / W;
        int wall = x % 50 == 25 && y % 100 != 10 && y % 100 != 60;
        cost[i] = wall ? 0 : (i * 1103515245 + 12345) % 2147483648 / 65536 % 9 + 1;
    }
    int64_t c, e;
    search(0, &c, &e);
    printf("dijkstra %lld %lld\n", (long long)c, (long long)e);
    search(1, &c, &e);
    printf("astar %lld %lld\n", (long long)c, (long long)e);
    int64_t cells = 1;
    for (int64_t v = N - 1; prev_[v] >= 0; v = prev_[v]) cells++;
    printf("path %lld\n", (long long)cells);
    return 0;
}
