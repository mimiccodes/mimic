// Count the points of a grid over [-2, 1] x [-1, 1] that stay in the
// Mandelbrot set for 200 iterations.
#include <stdio.h>
#include <stdint.h>

static int inside(double cr, double ci, int max_iter) {
    double zr = 0.0, zi = 0.0;
    for (int k = 0; k < max_iter; k++) {
        if (zr * zr + zi * zi > 4.0) return 0;
        double nr = zr * zr - zi * zi + cr;
        double ni = 2.0 * zr * zi + ci;
        zr = nr;
        zi = ni;
    }
    return 1;
}

int main(void) {
    const int64_t w = 1200, h = 800;
    int64_t count = 0;
    for (int64_t j = 0; j < h; j++) {
        double ci = -1.0 + 2.0 * (double)j / (double)h;
        for (int64_t i = 0; i < w; i++) {
            double cr = -2.0 + 3.0 * (double)i / (double)w;
            count += inside(cr, ci, 200);
        }
    }
    printf("%lld\n", (long long)count);
    return 0;
}
