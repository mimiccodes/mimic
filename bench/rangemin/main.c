// rangemin: range-minimum queries over 1,000,000 values (from splitmix64)
// with a sparse table (level k: the minimum of each run of 2^k values),
// 4,000,000 queries. Prints the sum of the answers and the sum of the
// query lengths.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 1000000
#define M 4000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int log2i(int64_t x) {
    int k = 0;
    while ((int64_t)2 << k <= x) k++;
    return k;
}

int main(void) {
    int levels = log2i(N) + 1;
    int64_t *t = malloc(sizeof(int64_t) * N * levels);
    for (int64_t i = 0; i < N; i++) t[i] = mix(i) % 1000000000;
    for (int k = 1; k < levels; k++) {
        int64_t h = (int64_t)1 << (k - 1), *a = t + (k - 1) * (int64_t)N, *b = t + k * (int64_t)N;
        for (int64_t i = 0; i < N; i++) {
            int64_t j = i + h < N - 1 ? i + h : N - 1;
            b[i] = a[i] < a[j] ? a[i] : a[j];
        }
    }
    int64_t sum = 0, lengths = 0;
    for (int64_t q = 0; q < M; q++) {
        int64_t u = mix(M + 2 * q) % N, v = mix(M + 2 * q + 1) % N;
        int64_t a = u < v ? u : v, b = u < v ? v : u;
        int k = log2i(b - a + 1);
        int64_t x = t[k * (int64_t)N + a], y = t[k * (int64_t)N + b - ((int64_t)1 << k) + 1];
        sum += x < y ? x : y;
        lengths += b - a + 1;
    }
    printf("%lld %lld\n", (long long)sum, (long long)lengths);
    return 0;
}
