// collatz: for every number below 3,000,000, the steps of its Collatz
// sequence (n/2 when even, 3n+1 when odd) until it reaches 1, each counted
// on its own. Prints the number with the longest sequence (the smallest,
// on a tie), its steps, and the sum of all steps.
#include <stdint.h>
#include <stdio.h>

int main(void) {
    int64_t n = 3000000, best = 1, most = 0, sum = 0;
    for (int64_t i = 1; i <= n; i++) {
        int64_t x = i, s = 0;
        while (x > 1) {
            x = x % 2 == 0 ? x / 2 : 3 * x + 1;
            s++;
        }
        sum += s;
        if (s > most) { most = s; best = i; }
    }
    printf("%lld %lld %lld\n", (long long)best, (long long)most, (long long)sum);
    return 0;
}
