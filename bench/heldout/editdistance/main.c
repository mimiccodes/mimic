// Edit distance (Levenshtein) between pairs of generated 100-letter strings
// over the letters a-e; prints the total distance.
#include <stdint.h>
#include <stdio.h>

#define PAIRS 2000
#define LEN 100

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static int64_t distance(const char *a, int n, const char *b, int m) {
    int64_t prev[LEN + 1], cur[LEN + 1];
    for (int j = 0; j <= m; j++) prev[j] = j;
    for (int i = 1; i <= n; i++) {
        cur[0] = i;
        for (int j = 1; j <= m; j++) {
            int64_t best = prev[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1);
            if (prev[j] + 1 < best) best = prev[j] + 1;
            if (cur[j - 1] + 1 < best) best = cur[j - 1] + 1;
            cur[j] = best;
        }
        for (int j = 0; j <= m; j++) prev[j] = cur[j];
    }
    return prev[m];
}

int main(void) {
    char a[LEN + 1], b[LEN + 1];
    int64_t total = 0;
    for (int p = 0; p < PAIRS; p++) {
        for (int i = 0; i < LEN; i++) a[i] = (char)('a' + next() % 5);
        for (int i = 0; i < LEN; i++) b[i] = (char)('a' + next() % 5);
        a[LEN] = b[LEN] = 0;
        total += distance(a, LEN, b, LEN);
    }
    printf("%lld\n", (long long)total);
    return 0;
}
