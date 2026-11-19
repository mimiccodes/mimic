// gauss: the rank and determinant of a 500 × 500 matrix modulo the prime
// 1,000,000,007 by Gaussian elimination, for a matrix of rank 499 and for
// the same matrix with its last row replaced (from splitmix64). Prints,
// for each matrix, the rank and the determinant.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 500
#define P 1000000007LL

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t power(int64_t b, int64_t e) {
    int64_t r = 1;
    for (b %= P; e; e >>= 1, b = b * b % P)
        if (e & 1) r = r * b % P;
    return r;
}

static void solve(int64_t *a, int64_t *rank_out, int64_t *det_out) {
    int64_t det = 1, rank = 0;
    for (int col = 0; col < N; col++) {
        int piv = -1;
        for (int r = (int)rank; r < N; r++)
            if (a[r * N + col]) {
                piv = r;
                break;
            }
        if (piv < 0) {
            det = 0;
            continue;
        }
        if (piv != rank) {
            for (int j = 0; j < N; j++) {
                int64_t t = a[rank * N + j];
                a[rank * N + j] = a[piv * N + j];
                a[piv * N + j] = t;
            }
            det = (P - det) % P;
        }
        int64_t lead = a[rank * N + col], inv = power(lead, P - 2);
        det = det * lead % P;
        for (int r = (int)rank + 1; r < N; r++) {
            int64_t f = a[r * N + col] * inv % P;
            if (!f) continue;
            for (int j = 0; j < N; j++) a[r * N + j] = (a[r * N + j] - f * a[rank * N + j] % P + P) % P;
        }
        rank++;
    }
    *rank_out = rank;
    *det_out = det;
}

int main(void) {
    int64_t *base = malloc(sizeof(int64_t) * N * N), *a = malloc(sizeof(int64_t) * N * N);
    for (int64_t k = 0; k < N * N; k++) base[k] = mix(k) % P;
    int64_t r1, d1, r2, d2;
    memcpy(a, base, sizeof(int64_t) * N * N);
    for (int j = 0; j < N; j++) a[(N - 1) * N + j] = (base[j] + base[N + j]) % P;
    solve(a, &r1, &d1);
    memcpy(a, base, sizeof(int64_t) * N * N);
    for (int j = 0; j < N; j++) a[(N - 1) * N + j] = mix(1000000 + j) % P;
    solve(a, &r2, &d2);
    printf("%lld %lld %lld %lld\n", (long long)r1, (long long)d1, (long long)r2, (long long)d2);
    return 0;
}
