// histeq: histogram equalization of six 3840×2160 8-bit frames of low
// contrast. Prints, per frame, the sum of the result and a position-weighted
// checksum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 3840
#define H 2160

int main(void) {
    int64_t n = (int64_t)W * H;
    uint8_t *img = malloc(n), *out = malloc(n);
    for (int64_t f = 0; f < 6; f++) {
        for (int64_t i = 0; i < n; i++) {
            int64_t x = i % W, y = i / W;
            int64_t noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            img[i] = (uint8_t)(64 + (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 128);
        }
        int64_t hist[256] = {0}, cdf[256], lut[256];
        for (int64_t i = 0; i < n; i++) hist[img[i]]++;
        int64_t run = 0, m = -1;
        for (int v = 0; v < 256; v++) {
            run += hist[v];
            cdf[v] = run;
            if (m < 0 && hist[v] > 0) m = run;
        }
        for (int v = 0; v < 256; v++) {
            int64_t q = (cdf[v] - m) * 255 + (n - m) / 2;
            int64_t d = n - m;
            int64_t l = q >= 0 ? q / d : -((-q + d - 1) / d);
            lut[v] = l > 0 ? l : 0;
        }
        int64_t sum = 0, check = 0;
        for (int64_t i = 0; i < n; i++) {
            out[i] = (uint8_t)lut[img[i]];
            sum += out[i];
            check += out[i] * (i % 251 + 1);
        }
        printf("%lld %lld %lld\n", (long long)f, (long long)sum, (long long)check);
    }
    free(img);
    free(out);
    return 0;
}
