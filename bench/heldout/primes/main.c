// Count the primes below a limit by trial division up to the square root.
#include <stdio.h>
#include <stdint.h>
#include <stdbool.h>

static bool is_prime(int64_t n) {
    if (n < 2) return false;
    if (n % 2 == 0) return n == 2;
    for (int64_t d = 3; d * d <= n; d += 2) {
        if (n % d == 0) return false;
    }
    return true;
}

int main(void) {
    const int64_t limit = 3000000;
    int64_t count = 0;
    for (int64_t n = 2; n < limit; n++) {
        if (is_prime(n)) count++;
    }
    printf("%lld\n", (long long)count);
    return 0;
}
