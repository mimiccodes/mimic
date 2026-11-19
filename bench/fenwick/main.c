// fenwick: a Fenwick tree over 1,048,576 counters: 1,000,000 updates (add
// 1 to 1,000 at a position, from splitmix64), then 2,000,000 prefix sums.
// Prints the sum of the prefix sums and the total.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define SIZE 1048576

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t prefix(const int64_t *t, int64_t i) {
    int64_t s = 0;
    for (; i > 0; i -= i & -i) s += t[i];
    return s;
}

int main(void) {
    int64_t *t = calloc(SIZE + 1, sizeof(int64_t));
    for (int64_t k = 0; k < 1000000; k++) {
        int64_t d = 1 + mix(2 * k + 1) % 1000;
        for (int64_t i = 1 + mix(2 * k) % SIZE; i <= SIZE; i += i & -i) t[i] += d;
    }
    int64_t total = 0;
    for (int64_t q = 0; q < 2000000; q++) total += prefix(t, 1 + mix(3000000 + q) % SIZE);
    printf("%lld %lld\n", (long long)total, (long long)prefix(t, SIZE));
    free(t);
    return 0;
}
