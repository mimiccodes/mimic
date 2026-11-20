#include <stdio.h>
#include <stdint.h>

// π from 200,000,000 points of the unit square (splitmix64 coordinates):
// the count inside the quarter circle, and 4 × count / points.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

int main(void) {
    const int64_t n = 200000000;
    int64_t count = 0;
    for (int64_t i = 0; i < n; i++) {
        double x = (double)(mix(2 * i) >> 10) / 9007199254740992.0;
        double y = (double)(mix(2 * i + 1) >> 10) / 9007199254740992.0;
        count += x * x + y * y < 1.0;
    }
    int64_t micro = count * 4000000 / n;
    printf("%lld %lld.%06lld\n", (long long)count, (long long)(micro / 1000000), (long long)(micro % 1000000));
    return 0;
}
