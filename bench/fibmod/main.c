// fibmod: F(n) modulo 1,000,000,007 for 500,000 values n below 10^18
// (from splitmix64), by fast doubling over the bits of n. Prints the sum
// of the results modulo the prime, and their exclusive or.
#include <stdint.h>
#include <stdio.h>

#define P 1000000007LL

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t fib(int64_t n) {
    int64_t a = 0, b = 1;
    for (int bit = 60; bit >= 0; bit--) {
        int64_t c = a * ((2 * b - a + P) % P) % P, d = (a * a + b * b) % P;
        if (n >> bit & 1) {
            a = d;
            b = (c + d) % P;
        } else {
            a = c;
            b = d;
        }
    }
    return a;
}

int main(void) {
    int64_t s = 0, x = 0;
    for (int64_t k = 0; k < 500000; k++) {
        int64_t f = fib(mix(k) % 1000000000000000000LL);
        s = (s + f) % P;
        x ^= f;
    }
    printf("%lld %lld\n", (long long)s, (long long)x);
    return 0;
}
