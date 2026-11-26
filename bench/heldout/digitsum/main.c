// For every n below a limit, sum its digit sum, and count the n whose
// digit sum is 30.
#include <stdio.h>
#include <stdint.h>

static int64_t digit_sum(int64_t n) {
    int64_t s = 0;
    while (n > 0) {
        s += n % 10;
        n /= 10;
    }
    return s;
}

int main(void) {
    const int64_t limit = 20000000;
    int64_t total = 0, thirty = 0;
    for (int64_t n = 0; n < limit; n++) {
        int64_t d = digit_sum(n);
        total += d;
        if (d == 30) thirty++;
    }
    printf("%lld\n%lld\n", (long long)total, (long long)thirty);
    return 0;
}
