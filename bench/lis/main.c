// lis: the longest strictly increasing subsequence of 4,000,000 values
// (from splitmix64) by patience sorting: each value replaces the first tail
// not below it (binary search) or extends the tails. Prints the length and
// the sum of the tails.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 4000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t *tails = malloc(sizeof(int64_t) * N), len = 0;
    for (int64_t k = 0; k < N; k++) {
        int64_t v = mix(k) % 1000000000, lo = 0, hi = len;
        while (lo < hi) {
            int64_t mid = (lo + hi) / 2;
            if (tails[mid] < v) lo = mid + 1;
            else hi = mid;
        }
        tails[lo] = v;
        if (lo == len) len++;
    }
    int64_t sum = 0;
    for (int64_t i = 0; i < len; i++) sum += tails[i];
    printf("%lld %lld\n", (long long)len, (long long)sum);
    free(tails);
    return 0;
}
