// ising: the 2D Ising model on a 512 × 512 torus at β = 0.44, 100 sweeps of
// Metropolis updates in checkerboard order, with random numbers from
// splitmix64 and the acceptance thresholds fixed as integers. Prints the
// magnetisation and the energy at the end.
#include <stdint.h>
#include <stdio.h>

#define L 512
#define N (L * L)

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t s[N];
#define AT(x, y) s[(((y) + L) % L) * L + ((x) + L) % L]

int main(void) {
    for (int64_t i = 0; i < N; i++) s[i] = mix(i) % 2 ? 1 : -1;
    for (int64_t h = 0; h < 200; h++)
        for (int64_t i = 0; i < N; i++) {
            int64_t x = i % L, y = i / L;
            if ((x + y) % 2 != h % 2) continue;
            int64_t de = 2 * s[i] * (AT(x - 1, y) + AT(x + 1, y) + AT(x, y - 1) + AT(x, y + 1));
            int64_t r = mix((1LL << 30) + h * N + i) % (1LL << 32);
            int64_t t = de == 4 ? 738927063 : de == 8 ? 127128606 : 0;
            if (de <= 0 || r < t) s[i] = -s[i];
        }
    int64_t m = 0, bonds = 0;
    for (int64_t i = 0; i < N; i++) {
        m += s[i];
        bonds += s[i] * (AT(i % L + 1, i / L) + AT(i % L, i / L + 1));
    }
    printf("%lld %lld\n", (long long)m, (long long)-bonds);
    return 0;
}
