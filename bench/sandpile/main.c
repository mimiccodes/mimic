// sandpile: an abelian sandpile on a 128 × 128 grid (start from
// splitmix64), toppled in synchronous rounds (each cell sends s div 4 to
// each neighbour, keeps s mod 4) until stable. Prints the number of rounds,
// the grains left, and how many cells end with 0, 1, 2 and 3.
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define L 128

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t s[L][L], t[L][L];

int main(void) {
    for (int i = 0; i < L * L; i++) s[i / L][i % L] = 4 + mix(i) % 30;
    int64_t rounds = 0;
    for (;;) {
        int64_t top = 0;
        for (int y = 0; y < L; y++)
            for (int x = 0; x < L; x++)
                if (s[y][x] > top) top = s[y][x];
        if (top < 4) break;
        for (int y = 0; y < L; y++)
            for (int x = 0; x < L; x++) {
                int64_t v = s[y][x] % 4;
                if (x > 0) v += s[y][x - 1] / 4;
                if (x < L - 1) v += s[y][x + 1] / 4;
                if (y > 0) v += s[y - 1][x] / 4;
                if (y < L - 1) v += s[y + 1][x] / 4;
                t[y][x] = v;
            }
        memcpy(s, t, sizeof s);
        rounds++;
    }
    int64_t sum = 0, c[4] = {0};
    for (int y = 0; y < L; y++)
        for (int x = 0; x < L; x++) {
            sum += s[y][x];
            c[s[y][x]]++;
        }
    printf("%lld %lld %lld %lld %lld %lld\n", (long long)rounds, (long long)sum, (long long)c[0], (long long)c[1], (long long)c[2], (long long)c[3]);
    return 0;
}
