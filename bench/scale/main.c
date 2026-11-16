/* scale: 30 frames of 1920x1080 8-bit pixels, each scaled (bilinear, 8-bit
 * fixed-point weights) to 1280x720, 854x480, 640x360 and 426x240; prints the
 * sum of every output pixel */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static void frame(uint8_t *p, long w, long h, long f) {
    for (long i = 0; i < w * h; i++) {
        long x = i % w, y = i / w;
        p[i] = (uint8_t)((x * 7 + y * 3 + x * y % 13 + f * 5) % 256);
    }
}

static void scale(const uint8_t *src, long w, long h, uint8_t *dst, long w2, long h2) {
    for (long i = 0; i < w2 * h2; i++) {
        long sx = i % w2 * (w - 1) * 256 / (w2 - 1);
        long sy = i / w2 * (h - 1) * 256 / (h2 - 1);
        long x0 = sx >> 8, y0 = sy >> 8, fx = sx & 255, fy = sy & 255;
        long x1 = x0 + 1 < w - 1 ? x0 + 1 : w - 1;
        long y1 = y0 + 1 < h - 1 ? y0 + 1 : h - 1;
        long top = src[y0 * w + x0] * (256 - fx) + src[y0 * w + x1] * fx;
        long bot = src[y1 * w + x0] * (256 - fx) + src[y1 * w + x1] * fx;
        dst[i] = (uint8_t)((top * (256 - fy) + bot * fy + 32768) >> 16);
    }
}

int main(void) {
    const long sizes[4][2] = {{1280, 720}, {854, 480}, {640, 360}, {426, 240}};
    uint8_t *src = malloc(1920 * 1080), *dst = malloc(1280 * 720);
    long long total = 0;
    for (long f = 0; f < 30; f++) {
        frame(src, 1920, 1080, f);
        for (int s = 0; s < 4; s++) {
            scale(src, 1920, 1080, dst, sizes[s][0], sizes[s][1]);
            for (long i = 0; i < sizes[s][0] * sizes[s][1]; i++) total += dst[i];
        }
    }
    printf("%lld\n", total);
    free(src);
    free(dst);
    return 0;
}
