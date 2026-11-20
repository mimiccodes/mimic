// numparse: 2,000,000 signed integers of 1 to 16 digits (from splitmix64)
// written one a line into one text, then read back with strtoll. Prints
// the sum, the number of negatives, the largest and the smallest.
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

int main(void) {
    char *text = malloc(20 * (size_t)N), *p = text;
    for (int64_t i = 0; i < N; i++) {
        int64_t m = 1;
        for (int64_t d = 1 + mix(i + 10000000) % 16; d > 0; d--) m *= 10;
        int64_t v = mix(i) % m;
        if (mix(i + 20000000) % 2) v = -v;
        if (i) *p++ = '\n';
        p += sprintf(p, "%lld", (long long)v);
    }
    int64_t sum = 0, neg = 0, hi = INT64_MIN, lo = INT64_MAX;
    for (char *q = text; *q;) {
        int64_t x = strtoll(q, &q, 10);
        sum += x;
        neg += x < 0;
        if (x > hi) hi = x;
        if (x < lo) lo = x;
        if (*q == '\n') q++;
    }
    printf("%lld %lld %lld %lld\n", (long long)sum, (long long)neg, (long long)hi, (long long)lo);
    return 0;
}
