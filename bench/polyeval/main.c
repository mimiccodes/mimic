// polyeval: 1,000,000 polynomials of degree 31 modulo 1,000,000,007
// (coefficients from splitmix64), each evaluated at its own point by
// Horner's rule. Prints the sum of the values modulo p and their exclusive
// or.
#include <stdint.h>
#include <stdio.h>

#define P 1000000007LL

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t s = 0, x = 0;
    for (int64_t k = 0; k < 1000000; k++) {
        int64_t at = mix(100000000 + k) % P, v = 0;
        for (int64_t j = 0; j < 32; j++) v = (v * at % P + mix(32 * k + j) % P) % P;
        s = (s + v) % P;
        x ^= v;
    }
    printf("%lld %lld\n", (long long)s, (long long)x);
    return 0;
}
