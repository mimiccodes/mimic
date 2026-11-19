// manacher: Manacher's algorithm over 2,000,000 letters from a 2-letter
// alphabet (from splitmix64), on the letters with a separator between each
// two. Prints the number of palindromic substrings, the longest one's
// length, and where the first of that length starts.
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
    int64_t m = 2 * N + 1;
    uint8_t *t = malloc(m);
    int64_t *p = calloc(m, sizeof(int64_t));
    for (int64_t i = 0; i < m; i++) t[i] = i % 2 ? (uint8_t)(mix(i / 2 / 2) % 2) : 2;
    int64_t c = 0, r = 0;
    for (int64_t i = 0; i < m; i++) {
        int64_t k = 0;
        if (i < r) k = r - i < p[2 * c - i] ? r - i : p[2 * c - i];
        while (i - k - 1 >= 0 && i + k + 1 < m && t[i - k - 1] == t[i + k + 1]) k++;
        p[i] = k;
        if (i + k > r) {
            c = i;
            r = i + k;
        }
    }
    int64_t count = 0, best = 0, at = 0;
    for (int64_t i = 0; i < m; i++) {
        count += (p[i] + 1) / 2;
        if (p[i] > best) {
            best = p[i];
            at = i;
        }
    }
    printf("%lld %lld %lld\n", (long long)count, (long long)best, (long long)((at - best) / 2));
    return 0;
}
