// totient: Euler's phi for every number up to 10,000,000 by a sieve.
// Prints the sum of phi over 1 … n, how many n have phi(n) = n - 1, and
// phi(10,000,000).
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 10000000

int main(void) {
    int64_t *phi = malloc(sizeof(int64_t) * (N + 1));
    for (int64_t i = 0; i <= N; i++) phi[i] = i;
    for (int64_t p = 2; p <= N; p++)
        if (phi[p] == p)
            for (int64_t m = p; m <= N; m += p) phi[m] -= phi[m] / p;
    int64_t sum = 0, primes = 0;
    for (int64_t i = 1; i <= N; i++) {
        sum += phi[i];
        primes += i >= 2 && phi[i] == i - 1;
    }
    printf("%lld %lld %lld\n", (long long)sum, (long long)primes, (long long)phi[N]);
    return 0;
}
