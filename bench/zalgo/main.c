// zalgo: the Z-array of 4,000,000 letters from a 4-letter alphabet (from
// splitmix64) by the Z-algorithm. Prints the sum of z, how many positions
// have z of 12 or more, and the largest z.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 4000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    uint8_t *s = malloc(N);
    int64_t *z = calloc(N, sizeof(int64_t));
    for (int64_t i = 0; i < N; i++) s[i] = (uint8_t)(mix(i / 3) % 4);
    int64_t l = 0, r = 0;
    for (int64_t i = 1; i < N; i++) {
        int64_t k = 0;
        if (i < r) k = r - i < z[i - l] ? r - i : z[i - l];
        while (i + k < N && s[k] == s[i + k]) k++;
        z[i] = k;
        if (i + k > r) {
            l = i;
            r = i + k;
        }
    }
    int64_t sum = 0, many = 0, most = 0;
    for (int64_t i = 0; i < N; i++) {
        sum += z[i];
        many += z[i] >= 12;
        if (z[i] > most) most = z[i];
    }
    printf("%lld %lld %lld\n", (long long)sum, (long long)many, (long long)most);
    return 0;
}
