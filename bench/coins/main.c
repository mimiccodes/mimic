// coins: the number of ways, modulo 1,000,000,007, to make every amount up
// to 3,000,000 from 30 kinds of coin (k*k + 1 for k below 30), coin by
// coin: ways[x] += ways[x - c]. Prints the ways for 3,000,000 and the sum
// of all the ways.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define A 3000000
#define P 1000000007

int main(void) {
    int64_t *ways = calloc(A + 1, sizeof(int64_t));
    ways[0] = 1;
    for (int64_t k = 0; k < 30; k++) {
        int64_t c = k * k + 1;
        for (int64_t x = c; x <= A; x++) ways[x] = (ways[x] + ways[x - c]) % P;
    }
    int64_t sum = 0;
    for (int64_t x = 0; x <= A; x++) sum = (sum + ways[x]) % P;
    printf("%lld %lld\n", (long long)ways[A], (long long)sum);
    free(ways);
    return 0;
}
