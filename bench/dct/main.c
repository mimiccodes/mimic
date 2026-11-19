// dct: the 8 × 8 two-dimensional DCT (as JPEG uses it) of every block of
// ten 1920 × 1080 8-bit frames, in fixed point (a cosine table scaled by
// 4096): the rows, then the columns, then >> 24 rounded down. Prints the
// sum of the absolute values of the coefficients and a position-weighted
// checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 1920
#define H 1080

static const int64_t C[64] = {1448, 1448, 1448, 1448, 1448, 1448, 1448, 1448, 2009, 1703, 1138, 400, -400, -1138, -1703, -2009,
                               1892, 784, -784, -1892, -1892, -784, 784, 1892, 1703, -400, -2009, -1138, 1138, 2009, 400, -1703,
                               1448, -1448, -1448, 1448, 1448, -1448, -1448, 1448, 1138, -2009, 400, 1703, -1703, -400, 2009, -1138,
                               784, -1892, 1892, -784, -784, 1892, -1892, 784, 400, -1138, 1703, -2009, 2009, -1703, 1138, -400};

int main(void) {
    int64_t n = (int64_t)W * H, *b = malloc(8 * n), *r = malloc(8 * n), *out = malloc(8 * n);
    int64_t total = 0, check = 0;
    for (int64_t f = 0; f < 10; f++) {
        for (int64_t i = 0; i < n; i++) {
            int64_t x = i % W, y = i / W;
            int64_t noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            b[i] = (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256 - 128;
        }
        for (int64_t i = 0; i < n; i++) {
            int64_t u = i % 8, start = i - u, s = 0;
            for (int x = 0; x < 8; x++) s += C[u * 8 + x] * b[start + x];
            r[i] = s;
        }
        for (int64_t i = 0; i < n; i++) {
            int64_t v = i / W % 8, top = i - v * W, s = 0;
            for (int y = 0; y < 8; y++) s += C[v * 8 + y] * r[top + y * W];
            out[i] = (s + (1 << 23)) >> 24;
        }
        for (int64_t i = 0; i < n; i++) {
            total += out[i] < 0 ? -out[i] : out[i];
            check += out[i] * (i % 251 + 1);
        }
    }
    printf("%lld %lld\n", (long long)total, (long long)check);
    return 0;
}
