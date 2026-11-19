// partitions: the partition numbers p(n) modulo 1,000,000,007 for n up to
// 200,000, by Euler's pentagonal number recurrence. Prints p(200,000) and
// the sum of all p(n) modulo the prime.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 200000
#define P 1000000007LL

int main(void) {
    int64_t *p = calloc(N + 1, sizeof(int64_t));
    p[0] = 1;
    for (int64_t m = 1; m <= N; m++) {
        int64_t s = 0;
        for (int64_t k = 1;; k++) {
            int64_t g1 = k * (3 * k - 1) / 2;
            if (g1 > m) break;
            int64_t g2 = k * (3 * k + 1) / 2;
            int64_t t = p[m - g1] + (g2 <= m ? p[m - g2] : 0);
            s += k % 2 ? t : -t;
        }
        p[m] = (s % P + P) % P;
    }
    int64_t sum = 0;
    for (int64_t m = 0; m <= N; m++) sum = (sum + p[m]) % P;
    printf("%lld %lld\n", (long long)p[N], (long long)sum);
    return 0;
}
