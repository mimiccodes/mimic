#include <stdio.h>
#include <stdint.h>

int main(void) {
    const int64_t n = 2000, limit = 200;
    int64_t count = 0;
    for (int64_t y = 0; y < n; y++)
        for (int64_t x = 0; x < n; x++) {
            double cr = -2.0 + 2.5 * (double)x / (double)n, ci = -1.25 + 2.5 * (double)y / (double)n;
            double zr = 0, zi = 0;
            int64_t k = 0;
            while (k < limit && zr * zr + zi * zi <= 4.0) {
                double t = zr * zr - zi * zi + cr;
                zi = 2.0 * zr * zi + ci;
                zr = t;
                k++;
            }
            count += k == limit;
        }
    printf("%lld\n", (long long)count);
    return 0;
}
