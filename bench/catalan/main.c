// catalan: the Catalan numbers C(0) … C(20,000) modulo 1,000,000,007 by
// the convolution C(n + 1) = sum of C(i) C(n - i). Prints C(20,000) and
// the sum of all of them modulo the prime.
#include <stdint.h>
#include <stdio.h>

#define N 20000
#define P 1000000007LL

int main(void) {
    static int64_t cs[N + 1];
    cs[0] = 1;
    for (int64_t n = 0; n < N; n++) {
        int64_t s = 0;
        for (int64_t i = 0; i <= n; i++) s += cs[i] * cs[n - i] % P;
        cs[n + 1] = s % P;
    }
    int64_t sum = 0;
    for (int64_t n = 0; n <= N; n++) sum = (sum + cs[n]) % P;
    printf("%lld %lld\n", (long long)cs[N], (long long)sum);
    return 0;
}
