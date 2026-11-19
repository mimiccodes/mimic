// traffic: the Nagel-Schreckenberg traffic model on a ring road of
// 1,000,000 cells with 200,000 cars and top speed 5, for 500 steps (random
// slowing from splitmix64), every car moving at once. Prints the total
// distance driven, the sum of the speeds at the end, and how many cars end
// standing.
#include <stdint.h>
#include <stdio.h>

#define L 1000000
#define C 200000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    static int64_t pos[C], v[C];
    for (int64_t c = 0; c < C; c++) pos[c] = 5 * c;
    int64_t driven = 0;
    for (int64_t t = 0; t < 500; t++) {
        for (int64_t c = 0; c < C; c++) {
            int64_t gap = (pos[(c + 1) % C] - pos[c] - 1 + L) % L, s = v[c] + 1;
            if (s > 5) s = 5;
            if (s > gap) s = gap;
            if (mix(C * t + c) % 4 == 0 && s > 0) s--;
            v[c] = s;
        }
        for (int64_t c = 0; c < C; c++) {
            pos[c] = (pos[c] + v[c]) % L;
            driven += v[c];
        }
    }
    int64_t sum = 0, standing = 0;
    for (int64_t c = 0; c < C; c++) {
        sum += v[c];
        standing += v[c] == 0;
    }
    printf("%lld %lld %lld\n", (long long)driven, (long long)sum, (long long)standing);
    return 0;
}
