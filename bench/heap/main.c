// heap: a binary min-heap in one array: 1,000,000 numbers below
// 1,000,000,000 (from splitmix64) pushed, then popped smallest first
// (heapsort). Prints the first, middle and last popped and the sum of each
// value times its position (wrapping).
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
    int64_t m = 1000000, n = 0;
    int64_t *a = malloc(sizeof(int64_t) * (size_t)m), *out = malloc(sizeof(int64_t) * (size_t)m);
    for (int64_t k = 0; k < m; k++) {
        int64_t i = n++;
        a[i] = mix(k) % 1000000000;
        while (i > 0 && a[(i - 1) / 2] > a[i]) {
            int64_t p = (i - 1) / 2, t = a[p];
            a[p] = a[i]; a[i] = t; i = p;
        }
    }
    for (int64_t k = 0; k < m; k++) {
        out[k] = a[0];
        a[0] = a[--n];
        int64_t i = 0;
        for (;;) {
            int64_t l = 2 * i + 1, r = l + 1, c;
            if (l >= n) break;
            c = r < n && a[r] < a[l] ? r : l;
            if (a[i] <= a[c]) break;
            int64_t t = a[c]; a[c] = a[i]; a[i] = t; i = c;
        }
    }
    uint64_t check = 0;
    for (int64_t i = 0; i < m; i++) check += (uint64_t)out[i] * (uint64_t)i;
    printf("%lld %lld %lld %lld\n", (long long)out[0], (long long)out[m / 2], (long long)out[m - 1], (long long)check);
    free(a);
    free(out);
    return 0;
}
