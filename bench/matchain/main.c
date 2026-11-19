// matchain: the cheapest order to multiply a chain of 600 matrices
// (dimensions from splitmix64) by the interval dynamic programme, by
// increasing length. Prints the cheapest cost of the whole chain and the
// sum of the cheapest costs of all sub-chains.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 600

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t d[N + 1], *cost = calloc(N * N, sizeof(int64_t));
    for (int i = 0; i <= N; i++) d[i] = 10 + mix(i) % 90;
    for (int len = 2; len <= N; len++)
        for (int i = 0; i + len - 1 < N; i++) {
            int j = i + len - 1;
            int64_t b = (int64_t)1 << 62;
            for (int k = i; k < j; k++) {
                int64_t c = cost[i * N + k] + cost[(k + 1) * N + j] + d[i] * d[k + 1] * d[j + 1];
                if (c < b) b = c;
            }
            cost[i * N + j] = b;
        }
    int64_t sum = 0;
    for (int64_t e = 0; e < N * N; e++) sum += cost[e];
    printf("%lld %lld\n", (long long)cost[N - 1], (long long)sum);
    return 0;
}
