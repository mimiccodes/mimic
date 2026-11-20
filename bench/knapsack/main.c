// knapsack: the best value from 2,000 items (weights 1 to 1,000, values 1
// to 10,000, from splitmix64) in a knapsack of capacity 100,000, each item
// at most once, by the usual table, one row per item (kept in one array,
// filled from the top down). Prints the best value and the best at half
// the capacity.
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
    int64_t n = 2000, cap = 100000;
    int64_t *best = calloc((size_t)cap + 1, sizeof(int64_t));
    for (int64_t i = 0; i < n; i++) {
        int64_t w = 1 + mix(2 * i) % 1000, v = 1 + mix(2 * i + 1) % 10000;
        for (int64_t c = cap; c >= w; c--)
            if (best[c - w] + v > best[c]) best[c] = best[c - w] + v;
    }
    printf("%lld %lld\n", (long long)best[cap], (long long)best[cap / 2]);
    free(best);
    return 0;
}
