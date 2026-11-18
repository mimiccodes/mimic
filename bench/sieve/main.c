#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// The primes below 100,000,000 by a segmented sieve (segments of 262,144):
// their count and the largest.
int main(void) {
    const int64_t n = 100000000, size = 262144;
    int64_t ps[1300], np = 0;
    for (int64_t k = 2; k <= 10000; k++) {
        int prime = 1;
        for (int64_t d = 2; d * d <= k; d++)
            if (k % d == 0) { prime = 0; break; }
        if (prime) ps[np++] = k;
    }
    uint8_t *f = malloc(size);
    int64_t count = 0, largest = -1;
    for (int64_t lo = 0; lo < n; lo += size) {
        int64_t hi = lo + size < n ? lo + size : n;
        memset(f, 1, hi - lo);
        for (int64_t i = 0; i < np && ps[i] * ps[i] < hi; i++) {
            int64_t p = ps[i], m = (lo + p - 1) / p * p;
            if (m < p * p) m = p * p;
            for (; m < hi; m += p) f[m - lo] = 0;
        }
        for (int64_t j = 0; j < hi - lo; j++)
            if (f[j] && lo + j >= 2) { count++; largest = lo + j; }
    }
    printf("%lld %lld\n", (long long)count, (long long)largest);
    free(f);
    return 0;
}
