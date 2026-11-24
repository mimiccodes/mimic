// Sum the proper divisors of every n below a limit (trial division up to
// the square root); print the perfect numbers and how many are abundant.
#include <stdio.h>
#include <stdint.h>

static int64_t divisor_sum(int64_t n) {
    if (n == 1) return 0;
    int64_t s = 1;
    for (int64_t d = 2; d * d <= n; d++) {
        if (n % d == 0) {
            s += d;
            int64_t e = n / d;
            if (e != d) s += e;
        }
    }
    return s;
}

int main(void) {
    const int64_t limit = 300000;
    int64_t abundant = 0;
    for (int64_t n = 1; n < limit; n++) {
        int64_t s = divisor_sum(n);
        if (s == n) printf("perfect %lld\n", (long long)n);
        if (s > n) abundant++;
    }
    printf("abundant %lld\n", (long long)abundant);
    return 0;
}
