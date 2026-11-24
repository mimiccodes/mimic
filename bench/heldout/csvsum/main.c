// Build CSV text of generated rows of 4 numbers, parse it back, and print
// each column's sum and the (first) row with the largest third column.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    const int64_t rows = 300000;
    const int64_t mods[4] = {1001, 100003, 999983, 99};
    int64_t x = 42;

    char *text = malloc((size_t)rows * 40 + 1);
    size_t len = 0;
    for (int64_t r = 0; r < rows; r++) {
        int64_t v[4];
        for (int k = 0; k < 4; k++) {
            x = (x * 1103515245 + 12345) % 2147483648;
            v[k] = x % mods[k];
        }
        len += (size_t)sprintf(text + len, "%lld,%lld,%lld,%lld\n",
                               (long long)v[0], (long long)v[1], (long long)v[2], (long long)v[3]);
    }

    int64_t sums[4] = {0, 0, 0, 0};
    int64_t best_row = -1, best[4] = {0, 0, 0, 0};
    char *p = text;
    char *end = text + len;
    int64_t r = 0;
    while (p < end) {
        int64_t v[4];
        for (int k = 0; k < 4; k++) {
            v[k] = strtoll(p, &p, 10);
            p++; // skip ',' or '\n'
        }
        for (int k = 0; k < 4; k++) sums[k] += v[k];
        if (best_row < 0 || v[2] > best[2]) {
            best_row = r;
            for (int k = 0; k < 4; k++) best[k] = v[k];
        }
        r++;
    }
    printf("%lld %lld %lld %lld\n", (long long)sums[0], (long long)sums[1], (long long)sums[2], (long long)sums[3]);
    printf("row %lld: %lld,%lld,%lld,%lld\n", (long long)best_row,
           (long long)best[0], (long long)best[1], (long long)best[2], (long long)best[3]);
    free(text);
    return 0;
}
