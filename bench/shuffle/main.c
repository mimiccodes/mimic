// shuffle: the numbers 0 … 9,999,999 shuffled by Fisher-Yates (swaps from
// splitmix64). Prints the first and the last number and a
// position-weighted checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 10000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t *a = malloc(sizeof(int64_t) * N);
    for (int64_t i = 0; i < N; i++) a[i] = i;
    for (int64_t i = N - 1; i > 0; i--) {
        int64_t j = mix(i) % (i + 1), t = a[i];
        a[i] = a[j];
        a[j] = t;
    }
    int64_t check = 0;
    for (int64_t i = 0; i < N; i++) check += a[i] * (i % 251 + 1);
    printf("%lld %lld %lld\n", (long long)a[0], (long long)a[N - 1], (long long)check);
    return 0;
}
