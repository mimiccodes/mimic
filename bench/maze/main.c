// maze: a 500 × 500 maze carved by a depth-first search with a stack (the
// way on chosen by splitmix64), then a breadth-first search from the
// top-left cell. Prints the distance to the bottom-right cell, the number
// of dead ends, and the sum of all distances.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 500
#define N (W * W)

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t step_to(int64_t c, int d) {
    int64_t x = c % W, y = c / W;
    switch (d) {
    case 0: return y == 0 ? -1 : c - W;
    case 1: return x == W - 1 ? -1 : c + 1;
    case 2: return y == W - 1 ? -1 : c + W;
    default: return x == 0 ? -1 : c - 1;
    }
}

int main(void) {
    static int64_t stack[N], opened[N], dist[N], q[N];
    static uint8_t seen[N];
    int64_t top = 1, step = 0;
    stack[0] = 0;
    seen[0] = 1;
    while (top) {
        int64_t c = stack[top - 1];
        int free[4], count = 0;
        for (int d = 0; d < 4; d++) {
            int64_t t = step_to(c, d);
            if (t >= 0 && !seen[t]) free[count++] = d;
        }
        if (count) {
            int d = free[mix(step) % count];
            int64_t t = step_to(c, d);
            opened[c] |= 1 << d;
            opened[t] |= 1 << ((d + 2) % 4);
            seen[t] = 1;
            stack[top++] = t;
        } else top--;
        step++;
    }
    for (int64_t c = 0; c < N; c++) dist[c] = -1;
    dist[0] = 0;
    int64_t head = 0, tail = 0;
    q[tail++] = 0;
    while (head < tail) {
        int64_t c = q[head++];
        for (int d = 0; d < 4; d++) {
            if (!(opened[c] >> d & 1)) continue;
            int64_t t = step_to(c, d);
            if (dist[t] < 0) {
                dist[t] = dist[c] + 1;
                q[tail++] = t;
            }
        }
    }
    int64_t dead = 0, sum = 0;
    for (int64_t c = 0; c < N; c++) {
        dead += __builtin_popcountll((uint64_t)opened[c]) == 1;
        sum += dist[c];
    }
    printf("%lld %lld %lld\n", (long long)dist[N - 1], (long long)dead, (long long)sum);
    return 0;
}
