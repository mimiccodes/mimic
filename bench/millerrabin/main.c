// millerrabin: how many of the 3,000,000 odd numbers just below 2^31 are
// prime, by the deterministic Miller-Rabin test with bases 2, 3, 5 and 7,
// and their sum.
#include <stdint.h>
#include <stdio.h>

static int64_t power(int64_t b, int64_t e, int64_t m) {
    int64_t r = 1;
    for (b %= m; e; e >>= 1, b = b * b % m)
        if (e & 1) r = r * b % m;
    return r;
}

static int prime(int64_t n) {
    int64_t d = n - 1, s = 0;
    while (d % 2 == 0) {
        d /= 2;
        s++;
    }
    static const int64_t bases[4] = {2, 3, 5, 7};
    for (int i = 0; i < 4; i++) {
        int64_t x = power(bases[i], d, n);
        if (x == 1 || x == n - 1) continue;
        int ok = 0;
        for (int64_t r = 1; r < s; r++) {
            x = x * x % n;
            if (x == n - 1) {
                ok = 1;
                break;
            }
        }
        if (!ok) return 0;
    }
    return 1;
}

int main(void) {
    int64_t top = ((int64_t)1 << 31) - 1, count = 0, sum = 0;
    for (int64_t k = 0; k < 3000000; k++) {
        int64_t n = top - 2 * k;
        if (prime(n)) {
            count++;
            sum += n;
        }
    }
    printf("%lld %lld\n", (long long)count, (long long)sum);
    return 0;
}
