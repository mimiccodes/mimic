// coprime: over all pairs (i, j) with 1 <= i, j <= 4,000, how many have
// gcd 1, and the sum of all the gcds, by Euclid's algorithm.
#include <stdint.h>
#include <stdio.h>

static int64_t gcd(int64_t a, int64_t b) {
    while (b) {
        int64_t t = a % b;
        a = b;
        b = t;
    }
    return a;
}

int main(void) {
    int64_t ones = 0, sum = 0;
    for (int64_t i = 1; i <= 4000; i++)
        for (int64_t j = 1; j <= 4000; j++) {
            int64_t g = gcd(i, j);
            ones += g == 1;
            sum += g;
        }
    printf("%lld %lld\n", (long long)ones, (long long)sum);
    return 0;
}
