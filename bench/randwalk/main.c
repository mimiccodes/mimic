// randwalk: 200,000 walkers, each taking 1,000 steps on the square lattice
// from (0, 0) in directions from splitmix64. Prints the sum of the squared
// distances at the end, the largest, and how many walkers end where they
// started.
#include <stdint.h>
#include <stdio.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t sum = 0, most = 0, home = 0;
    for (int64_t w = 0; w < 200000; w++) {
        int64_t x = 0, y = 0;
        for (int64_t t = 0; t < 1000; t++) {
            int64_t d = mix(1000 * w + t) % 4;
            x += (d == 0) - (d == 2);
            y += (d == 1) - (d == 3);
        }
        int64_t r = x * x + y * y;
        sum += r;
        if (r > most) most = r;
        home += r == 0;
    }
    printf("%lld %lld %lld\n", (long long)sum, (long long)most, (long long)home);
    return 0;
}
