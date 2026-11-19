// flood: a breadth-first flood fill on a 2,000 × 2,000 grid with 30% walls
// (from splitmix64), from the first open cell at or after the centre.
// Prints how many cells are reached, the sum of their distances from the
// start, and the largest distance.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 2000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t n = (int64_t)W * W;
    uint8_t *wall = malloc(n);
    int64_t *dist = malloc(sizeof(int64_t) * n), *q = malloc(sizeof(int64_t) * n);
    for (int64_t i = 0; i < n; i++) {
        wall[i] = mix(i) % 100 < 30;
        dist[i] = -1;
    }
    int64_t start = (int64_t)W * 1000 + 1000;
    while (wall[start]) start++;
    int64_t head = 0, tail = 0;
    q[tail++] = start;
    dist[start] = 0;
    while (head < tail) {
        int64_t c = q[head++], x = c % W, y = c / W;
        int64_t nx[4] = {x - 1, x + 1, x, x}, ny[4] = {y, y, y - 1, y + 1};
        for (int k = 0; k < 4; k++) {
            if (nx[k] < 0 || ny[k] < 0 || nx[k] >= W || ny[k] >= W) continue;
            int64_t nc = ny[k] * W + nx[k];
            if (wall[nc] || dist[nc] >= 0) continue;
            dist[nc] = dist[c] + 1;
            q[tail++] = nc;
        }
    }
    int64_t reached = 0, sum = 0, most = 0;
    for (int64_t i = 0; i < n; i++)
        if (dist[i] >= 0) {
            reached++;
            sum += dist[i];
            if (dist[i] > most) most = dist[i];
        }
    printf("%lld %lld %lld\n", (long long)reached, (long long)sum, (long long)most);
    return 0;
}
