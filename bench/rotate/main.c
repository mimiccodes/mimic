// rotate: six 3840×2160 8-bit frames, each turned 90° clockwise four times;
// after each turn, a position-weighted checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 3840
#define H 2160

// a w×h image turned clockwise into out (h wide, w high)
static void turn(const uint8_t *img, uint8_t *out, int64_t w, int64_t h) {
    for (int64_t y = 0; y < w; y++)
        for (int64_t x = 0; x < h; x++) out[y * h + x] = img[(h - 1 - x) * w + y];
}

static int64_t check(const uint8_t *img, int64_t n) {
    int64_t s = 0;
    for (int64_t i = 0; i < n; i++) s += img[i] * (i % 251 + 1);
    return s;
}

int main(void) {
    int64_t n = (int64_t)W * H;
    uint8_t *a = malloc(n), *b = malloc(n);
    for (int64_t f = 0; f < 6; f++) {
        for (int64_t i = 0; i < n; i++) {
            int64_t x = i % W, y = i / W;
            int64_t noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            a[i] = (uint8_t)((x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256);
        }
        int64_t s[4];
        for (int k = 0; k < 4; k++) {
            int64_t w = k % 2 ? H : W, h = k % 2 ? W : H;
            turn(a, b, w, h);
            s[k] = check(b, n);
            uint8_t *t = a;
            a = b;
            b = t;
        }
        printf("%lld %lld %lld %lld %lld\n", (long long)f, (long long)s[0], (long long)s[1], (long long)s[2], (long long)s[3]);
    }
    free(a);
    free(b);
    return 0;
}
