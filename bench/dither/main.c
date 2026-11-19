// dither: 24 frames of 1280×720 8-bit pixels, each dithered to black and
// white by Floyd–Steinberg error diffusion (shifts floor). Prints, per
// frame, the number of white pixels and a position-weighted checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 1280
#define H 720

int main(void) {
    int64_t n = (int64_t)W * H;
    int64_t *buf = malloc(sizeof(int64_t) * n);
    for (int64_t f = 0; f < 24; f++) {
        for (int64_t i = 0; i < n; i++) {
            int64_t x = i % W, y = i / W;
            int64_t noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            buf[i] = (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256;
        }
        int64_t white = 0, check = 0;
        for (int64_t i = 0; i < n; i++) {
            int64_t old = buf[i], nw = old >= 128 ? 255 : 0, e = old - nw, x = i % W;
            if (x + 1 < W) buf[i + 1] += e * 7 >> 4;
            if (i + W < n) {
                if (x > 0) buf[i + W - 1] += e * 3 >> 4;
                buf[i + W] += e * 5 >> 4;
                if (x + 1 < W) buf[i + W + 1] += e >> 4;
            }
            if (nw) {
                white++;
                check += i % 251 + 1;
            }
        }
        printf("%lld %lld %lld\n", (long long)f, (long long)white, (long long)check);
    }
    free(buf);
    return 0;
}
