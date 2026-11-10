/* The same five algorithms as ../src/bench.mim, in plain sequential C, at the
 * same sizes as src/main.mim. Used by `make compare` as a speed reference.
 *
 * Built with -ffp-contract=off: no fused multiply-add, the same strict IEEE
 * arithmetic Mimic uses, so both print identical results. */
#include <stdint.h>
#include <stdio.h>

static int64_t escape(double cx, double cy, int64_t limit) {
    double x = 0, y = 0;
    int64_t i = 0;
    while (!(i == limit || x * x + y * y > 4.0)) {
        double nx = x * x - y * y + cx;
        y = 2.0 * x * y + cy;
        x = nx;
        i++;
    }
    return i;
}

static int64_t mandelbrot(int64_t size, int64_t limit) {
    int64_t total = 0;
    for (int64_t k = 0; k < size * size; k++) {
        double cx = (double)(k % size) / (double)size * 3.0 - 2.0;
        double cy = (double)(k / size) / (double)size * 3.0 - 1.5;
        total += escape(cx, cy, limit);
    }
    return total;
}

static int is_prime(int64_t n) {
    if (n < 2) return 0;
    for (int64_t d = 2; d * d <= n; d++)
        if (n % d == 0) return 0;
    return 1;
}

static int64_t chain(int64_t n) {
    int64_t c = 1;
    while (n != 1) {
        n = (n % 2 == 0) ? n / 2 : 3 * n + 1;
        c++;
    }
    return c;
}

int main(void) {
    printf("mandelbrot iterations (1000×1000, limit 500): %lld\n", (long long)mandelbrot(1000, 500));

    int64_t primes = 0;
    for (int64_t n = 0; n < 3000000; n++) primes += is_prime(n);
    printf("primes below 3,000,000: %lld\n", (long long)primes);

    int64_t best = 0;
    for (int64_t n = 1; n < 3000000; n++) {
        int64_t c = chain(n);
        if (c > best) best = c;
    }
    printf("longest Collatz chain below 3,000,000: %lld\n", (long long)best);

    int64_t inside = 0, n = 10000000;
    for (int64_t k = 0; k < n; k++) {
        double x = (double)((k * 1103515245 + 12345) % 2147483648) / 2147483648.0;
        double y = (double)((k * 22695477 + 1) % 2147483647) / 2147483647.0;
        inside += x * x + y * y <= 1.0;
    }
    printf("π by Monte Carlo (10M points): %.15g\n", 4.0 * (double)inside / (double)n);

    double sum = 0;
    int64_t m = 20000000;
    for (int64_t k = 0; k < m; k++) {
        double x = ((double)k + 0.5) / (double)m;
        sum += 4.0 / (1.0 + x * x);
    }
    printf("π by integration (20M slices): %.15g\n", sum / (double)m);
    return 0;
}
