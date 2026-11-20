// intervals: 2,000,000 intervals (from splitmix64) sorted by start and
// merged where they overlap or touch. Prints the number of merged
// intervals, the total length they cover, and the longest.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 2000000

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

int main(void) {
    int64_t *keys = malloc(sizeof(int64_t) * N);
    for (int64_t k = 0; k < N; k++) keys[k] = (mix(2 * k) % 1000000000) * 1024 + mix(2 * k + 1) % 1000;
    qsort(keys, N, sizeof(int64_t), cmp);
    int64_t count = 0, total = 0, longest = 0, cs = -1, ce = -1;
    for (int64_t i = 0; i < N; i++) {
        int64_t s = keys[i] / 1024, e = s + keys[i] % 1024 + 1;
        if (count && s <= ce) {
            if (e > ce) ce = e;
        } else {
            if (count) {
                total += ce - cs;
                if (ce - cs > longest) longest = ce - cs;
            }
            cs = s;
            ce = e;
            count++;
        }
    }
    total += ce - cs;
    if (ce - cs > longest) longest = ce - cs;
    printf("%lld %lld %lld\n", (long long)count, (long long)total, (long long)longest);
    return 0;
}
