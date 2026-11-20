// bsearch: 1,000,000 sorted keys (from splitmix64) and 4,000,000 lookups,
// each the lower bound by binary search (every 16th asks for a key that is
// there). Prints the sum of the positions and how many lookups found their
// key.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 1000000
#define M 4000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

// the first position whose key is not below q
static int64_t lower(const int64_t *keys, int64_t q) {
    int64_t lo = 0, hi = N;
    while (lo < hi) {
        int64_t mid = (lo + hi) / 2;
        if (keys[mid] < q) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

int main(void) {
    int64_t *keys = malloc(sizeof(int64_t) * N);
    for (int64_t k = 0; k < N; k++) keys[k] = mix(k) % 1000000000000;
    qsort(keys, N, sizeof(int64_t), cmp);
    int64_t sum = 0, found = 0;
    for (int64_t j = 0; j < M; j++) {
        int64_t q = j % 16 == 0 ? keys[(j / 16 * 7919) % N] : mix(1000000000 + j) % 1000000000000;
        int64_t p = lower(keys, q);
        sum += p;
        found += p < N && keys[p] == q;
    }
    printf("%lld %lld\n", (long long)sum, (long long)found);
    free(keys);
    return 0;
}
