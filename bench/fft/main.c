#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <math.h>

// The product of two polynomials of 524,288 coefficients (0 to 99, from
// splitmix64) by a radix-2 FFT of 2^20 points, rounded back to whole
// numbers: the sum of its coefficients, a checksum and the middle one.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

static void transform(double *re, double *im, int64_t n, const double *tr, const double *ti, double sign) {
    for (int64_t i = 1, j = 0; i < n; i++) {
        int64_t bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) {
            double t = re[i]; re[i] = re[j]; re[j] = t;
            t = im[i]; im[i] = im[j]; im[j] = t;
        }
    }
    for (int64_t h = 1; h < n; h *= 2) {
        int64_t stride = n / (2 * h);
        for (int64_t s = 0; s < n; s += 2 * h)
            for (int64_t k = 0; k < h; k++) {
                double wr = tr[k * stride], wi = sign * ti[k * stride];
                int64_t a = s + k, b = s + k + h;
                double xr = wr * re[b] - wi * im[b], xi = wr * im[b] + wi * re[b];
                re[b] = re[a] - xr; im[b] = im[a] - xi;
                re[a] += xr; im[a] += xi;
            }
    }
}

int main(void) {
    const int64_t n = 1 << 20, half = n / 2;
    double *tr = malloc(sizeof(double) * half), *ti = malloc(sizeof(double) * half);
    for (int64_t k = 0; k < half; k++) { tr[k] = cos(2.0 * M_PI * k / n); ti[k] = sin(2.0 * M_PI * k / n); }
    double *ar = calloc(n, sizeof(double)), *ai = calloc(n, sizeof(double));
    double *br = calloc(n, sizeof(double)), *bi = calloc(n, sizeof(double));
    for (int64_t i = 0; i < half; i++) { ar[i] = mix(i) % 100; br[i] = mix(n + i) % 100; }
    transform(ar, ai, n, tr, ti, -1.0);
    transform(br, bi, n, tr, ti, -1.0);
    for (int64_t p = 0; p < n; p++) {
        double r = ar[p] * br[p] - ai[p] * bi[p], i = ar[p] * bi[p] + ai[p] * br[p];
        ar[p] = r; ai[p] = i;
    }
    transform(ar, ai, n, tr, ti, 1.0);
    int64_t sum = 0, check = 0, mid = 0;
    for (int64_t i = 0; i < n; i++) {
        int64_t c = (int64_t)round(ar[i] / n);
        sum += c;
        check += c % 1000 * (i % 1000);
        if (i == half - 1) mid = c;
    }
    printf("%lld %lld %lld\n", (long long)sum, (long long)check, (long long)mid);
    return 0;
}
