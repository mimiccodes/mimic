// Sum gcd(i, j) over all 1 <= i, j <= n with Euclid's algorithm.
#include <stdio.h>
#include <stdint.h>

static int64_t gcd(int64_t a, int64_t b) {
    while (b != 0) {
        int64_t t = a % b;
        a = b;
        b = t;
    }
    return a;
}

int main(void) {
    const int64_t n = 2000;
    int64_t total = 0;
    for (int64_t i = 1; i <= n; i++) {
        for (int64_t j = 1; j <= n; j++) {
            total += gcd(i, j);
        }
    }
    printf("%lld\n", (long long)total);
    return 0;
}
