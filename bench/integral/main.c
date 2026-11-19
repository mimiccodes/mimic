// integral: a 2048×2048 8-bit image, its summed-area table (with a row and
// a column of zeros in front), and 8,000,000 box sums from it (corners from
// splitmix64). Prints the total of the box sums and the image's sum.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 2048
#define H 2048

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    uint8_t *img = malloc(W * H);
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W;
        int64_t noise = (i * 1103515245 + 12345) % 2147483648 / 65536 % 32;
        img[i] = (uint8_t)((x * 7 + y * 13 + x * y % 31 + noise) % 256);
    }
    int64_t *sat = calloc((W + 1) * (H + 1), sizeof(int64_t));
    for (int64_t y = 1; y <= H; y++) {
        int64_t row = 0;
        for (int64_t x = 1; x <= W; x++) {
            row += img[(y - 1) * W + x - 1];
            sat[y * (W + 1) + x] = sat[(y - 1) * (W + 1) + x] + row;
        }
    }
    int64_t total = 0;
    for (int64_t q = 0; q < 8000000; q++) {
        int64_t a = mix(4 * q) % W, b = mix(4 * q + 1) % W, c = mix(4 * q + 2) % H, d = mix(4 * q + 3) % H;
        int64_t x1 = a < b ? a : b, x2 = a < b ? b : a, y1 = c < d ? c : d, y2 = c < d ? d : c;
        total += sat[(y2 + 1) * (W + 1) + x2 + 1] - sat[y1 * (W + 1) + x2 + 1] - sat[(y2 + 1) * (W + 1) + x1] + sat[y1 * (W + 1) + x1];
    }
    printf("%lld %lld\n", (long long)total, (long long)sat[(W + 1) * (H + 1) - 1]);
    free(img);
    free(sat);
    return 0;
}
