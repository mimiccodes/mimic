// select: 20,000,000 values (from splitmix64) and their k-th smallest for
// k at 1%, 10%, 50%, 90% and 99%, each by quickselect on a fresh copy:
// the pivot is the middle element, a three-way partition in place, then
// the side that holds k. Prints the five values.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 20000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t select_k(int64_t *a, int64_t n, int64_t k) {
    for (;;) {
        int64_t p = a[n / 2];
        // a[0…lt) < p, a[lt…i) == p, a[gt…n) > p
        int64_t lt = 0, i = 0, gt = n;
        while (i < gt) {
            int64_t x = a[i];
            if (x < p) {
                a[i++] = a[lt];
                a[lt++] = x;
            } else if (x > p) {
                a[i] = a[--gt];
                a[gt] = x;
            } else i++;
        }
        if (k < lt) n = lt;
        else if (k < gt) return p;
        else {
            a += gt;
            n -= gt;
            k -= gt;
        }
    }
}

int main(void) {
    int64_t *xs = malloc(sizeof(int64_t) * N), *work = malloc(sizeof(int64_t) * N);
    for (int64_t k = 0; k < N; k++) xs[k] = mix(k) % 1000000000;
    int ps[5] = {1, 10, 50, 90, 99};
    for (int j = 0; j < 5; j++) {
        memcpy(work, xs, sizeof(int64_t) * N);
        printf(j ? " %lld" : "%lld", (long long)select_k(work, N, (int64_t)N * ps[j] / 100));
    }
    printf("\n");
    free(xs);
    free(work);
    return 0;
}
