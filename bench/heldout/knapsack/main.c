// 0/1 knapsack by dynamic programming, one table row per item; prints the
// best total value that fits in the capacity.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define ITEMS 1000
#define CAPACITY 100000

int main(void) {
    int64_t weight[ITEMS], value[ITEMS];
    int64_t x = 42;
    for (int i = 0; i < ITEMS; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        weight[i] = x % 1000 + 1;
        x = (x * 1103515245 + 12345) % 2147483648;
        value[i] = x % 1000 + 1;
    }
    int64_t *prev = calloc(CAPACITY + 1, sizeof *prev);
    int64_t *cur = calloc(CAPACITY + 1, sizeof *cur);
    for (int i = 0; i < ITEMS; i++) {
        for (int c = 0; c <= CAPACITY; c++) {
            int64_t best = prev[c];
            if (weight[i] <= c && prev[c - weight[i]] + value[i] > best)
                best = prev[c - weight[i]] + value[i];
            cur[c] = best;
        }
        int64_t *t = prev; prev = cur; cur = t;
    }
    printf("%lld\n", (long long)prev[CAPACITY]);
    free(prev);
    free(cur);
    return 0;
}
