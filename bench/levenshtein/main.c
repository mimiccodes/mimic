// levenshtein: the edit distance of 20,000 pairs of generated DNA-like
// strings (30 to 60 letters of "acgt"), by the usual dynamic program, one
// row after another. Prints the sum of the distances and the largest.
#include <stdint.h>
#include <stdio.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int word(int64_t k, int8_t *w) {
    int n = 30 + (int)(mix(k * 1000) % 31);
    for (int i = 0; i < n; i++) w[i] = (int8_t)(mix(k * 1000 + 1 + i) % 4);
    return n;
}

int main(void) {
    int8_t a[64], b[64];
    int prev[64], cur[64];
    int64_t sum = 0, most = 0;
    for (int64_t p = 0; p < 20000; p++) {
        int n = word(2 * p, a), m = word(2 * p + 1, b);
        for (int j = 0; j <= m; j++) prev[j] = j;
        for (int i = 1; i <= n; i++) {
            cur[0] = i;
            for (int j = 1; j <= m; j++) {
                int v = prev[j] + 1, l = cur[j - 1] + 1, d = prev[j - 1] + (a[i - 1] != b[j - 1]);
                if (l < v) v = l;
                if (d < v) v = d;
                cur[j] = v;
            }
            for (int j = 0; j <= m; j++) prev[j] = cur[j];
        }
        sum += prev[m];
        if (prev[m] > most) most = prev[m];
    }
    printf("%lld %lld\n", (long long)sum, (long long)most);
    return 0;
}
