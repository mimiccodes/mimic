#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

// The product of two 120,000-digit numbers (30,000 limbs of four decimal
// digits, from splitmix64) by schoolbook multiplication: the product's
// limbs, their sum, the top limb and the middle one.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    const int64_t n = 30000;
    int64_t *a = malloc(sizeof(int64_t) * n), *b = malloc(sizeof(int64_t) * n), *c = calloc(2 * n, sizeof(int64_t));
    for (int64_t i = 0; i < n; i++) {
        a[i] = i == n - 1 ? 1 + mix(i) % 9999 : mix(i) % 10000;
        b[i] = i == n - 1 ? 1 + mix(n + i) % 9999 : mix(n + i) % 10000;
    }
    for (int64_t i = 0; i < n; i++)
        for (int64_t j = 0; j < n; j++) c[i + j] += a[i] * b[j];
    for (int64_t k = 0; k < 2 * n - 1; k++) {
        c[k + 1] += c[k] / 10000;
        c[k] %= 10000;
    }
    int64_t size = c[2 * n - 1] == 0 ? 2 * n - 1 : 2 * n, sum = 0;
    for (int64_t k = 0; k < size; k++) sum += c[k];
    printf("%lld %lld %lld %lld\n", (long long)size, (long long)sum, (long long)c[size - 1], (long long)c[n]);
    return 0;
}
