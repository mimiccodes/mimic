// diff: the shortest edit script between 1,000 pairs of generated
// "files" by Myers' algorithm (the furthest x on each diagonal for each
// number of edits d, following runs of equal lines). Prints the sum of
// the edit lengths and the largest.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t myers(const int64_t *a, int64_t n, const int64_t *b, int64_t m, int64_t *v) {
    int64_t off = n + m + 1;
    for (int64_t i = 0; i < 2 * off + 1; i++) v[i] = 0;
    for (int64_t d = 0; d <= n + m; d++)
        for (int64_t k = -d; k <= d; k += 2) {
            int64_t x = (k == -d || (k != d && v[off + k - 1] < v[off + k + 1])) ? v[off + k + 1] : v[off + k - 1] + 1;
            int64_t y = x - k;
            while (x < n && y < m && a[x] == b[y]) x++, y++;
            v[off + k] = x;
            if (x >= n && y >= m) return d;
        }
    return -1;
}

int main(void) {
    int64_t *a = malloc(sizeof(int64_t) * 2001), *b = malloc(sizeof(int64_t) * 4002), *v = malloc(sizeof(int64_t) * 12010);
    int64_t sum = 0, most = 0;
    for (int64_t p = 0; p < 1000; p++) {
        int64_t n = 1000 + mix(p * 10) % 1001, m = 0;
        for (int64_t i = 0; i < n; i++) a[i] = mix(p * 100000 + i) % 50;
        for (int64_t i = 0; i < n; i++) {
            int64_t r = mix(p * 100000 + 50000 + i) % 100;
            if (r < 3) continue;
            if (r < 6) b[m++] = mix(p * 100000 + 70000 + i) % 50;
            else if (r < 9) { b[m++] = mix(p * 100000 + 90000 + i) % 50; b[m++] = a[i]; }
            else b[m++] = a[i];
        }
        int64_t d = myers(a, n, b, m, v);
        sum += d;
        if (d > most) most = d;
    }
    printf("%lld %lld\n", (long long)sum, (long long)most);
    free(a); free(b); free(v);
    return 0;
}
