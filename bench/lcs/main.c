// lcs: the longest common subsequence of two sequences of 20,000 letters
// from a 4-letter alphabet (from splitmix64), by the row-by-row table.
// Prints the length and the sum of the last row.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 20000
#define M 20000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t *a = malloc(sizeof(int64_t) * N), *b = malloc(sizeof(int64_t) * M);
    int64_t *prev = calloc(M + 1, sizeof(int64_t)), *row = calloc(M + 1, sizeof(int64_t));
    for (int64_t i = 0; i < N; i++) a[i] = mix(i) % 4;
    for (int64_t j = 0; j < M; j++) b[j] = mix(1000000 + j) % 4;
    for (int64_t i = 0; i < N; i++) {
        for (int64_t j = 1; j <= M; j++) {
            if (a[i] == b[j - 1]) row[j] = prev[j - 1] + 1;
            else row[j] = prev[j] > row[j - 1] ? prev[j] : row[j - 1];
        }
        int64_t *t = prev;
        prev = row;
        row = t;
    }
    int64_t sum = 0;
    for (int64_t j = 0; j <= M; j++) sum += prev[j];
    printf("%lld %lld\n", (long long)prev[M], (long long)sum);
    return 0;
}
