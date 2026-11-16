/* filters: a 1920x1080 8-bit image through five filters (edges clamped):
 * 3x3 box blur, 5x5 Gaussian, Sobel (|gx|+|gy|, at most 255), 3x3 median,
 * and an integer bilateral filter (3x3, spatial weights 1 2 4, range weight
 * max(0, 32 - |q - p| / 2)). Prints each result's sum and a checksum that
 * weighs every pixel by its position. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define W 1920
#define H 1080

static uint8_t img[W * H], out[W * H];

static inline int64_t at(int64_t x, int64_t y) {
    x = x < 0 ? 0 : x >= W ? W - 1 : x;
    y = y < 0 ? 0 : y >= H ? H - 1 : y;
    return img[y * W + x];
}

static void report(const char *name) {
    int64_t sum = 0, check = 0;
    for (int64_t i = 0; i < W * H; i++) {
        sum += out[i];
        check += (int64_t)out[i] * (i % 251 + 1);
    }
    printf("%s %lld %lld\n", name, (long long)sum, (long long)check);
}

int main(void) {
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W;
        int64_t noise = (i * 1103515245 + 12345) % 2147483648 / 65536 % 32;
        img[i] = (uint8_t)((x * 7 + y * 13 + x * y % 31 + noise) % 256);
    }
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W, s = 0;
        for (int64_t j = 0; j < 9; j++) s += at(x + j % 3 - 1, y + j / 3 - 1);
        out[i] = (uint8_t)(s / 9);
    }
    report("box");
    static const int64_t g[5] = {1, 4, 6, 4, 1};
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W, s = 0;
        for (int64_t j = 0; j < 25; j++) s += g[j / 5] * g[j % 5] * at(x + j % 5 - 2, y + j / 5 - 2);
        out[i] = (uint8_t)(s / 256);
    }
    report("gaussian");
    static const int64_t sx[9] = {-1, 0, 1, -2, 0, 2, -1, 0, 1}, sy[9] = {-1, -2, -1, 0, 0, 0, 1, 2, 1};
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W, a = 0, b = 0;
        for (int64_t j = 0; j < 9; j++) {
            int64_t p = at(x + j % 3 - 1, y + j / 3 - 1);
            a += sx[j] * p;
            b += sy[j] * p;
        }
        int64_t m = llabs(a) + llabs(b);
        out[i] = (uint8_t)(m > 255 ? 255 : m);
    }
    report("sobel");
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W, v[9];
        for (int64_t j = 0; j < 9; j++) v[j] = at(x + j % 3 - 1, y + j / 3 - 1);
        for (int64_t a = 1; a < 9; a++) {
            int64_t t = v[a], b = a - 1;
            while (b >= 0 && v[b] > t) { v[b + 1] = v[b]; b--; }
            v[b + 1] = t;
        }
        out[i] = (uint8_t)v[4];
    }
    report("median");
    static const int64_t sw[9] = {1, 2, 1, 2, 4, 2, 1, 2, 1};
    for (int64_t i = 0; i < W * H; i++) {
        int64_t x = i % W, y = i / W, p = img[i], num = 0, den = 0;
        for (int64_t j = 0; j < 9; j++) {
            int64_t q = at(x + j % 3 - 1, y + j / 3 - 1);
            int64_t r = 32 - llabs(q - p) / 2;
            int64_t w = sw[j] * (r > 0 ? r : 0);
            num += w * q;
            den += w;
        }
        out[i] = (uint8_t)((num + den / 2) / den);
    }
    report("bilateral");
    return 0;
}
