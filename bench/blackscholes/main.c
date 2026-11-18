#include <stdio.h>
#include <stdint.h>
#include <math.h>

// 20,000,000 European options priced by Black-Scholes (the normal
// distribution as in Abramowitz and Stegun 26.2.17), inputs from
// splitmix64: the total of every call and put, each rounded to cents.
static uint64_t mix(uint64_t v) {
    uint64_t x = (v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (x ^ x >> 31) >> 1;
}

static double unit(uint64_t v) { return (double)(mix(v) >> 10) / 9007199254740992.0; }

static double cnd(double x) {
    double k = 1.0 / (1.0 + 0.2316419 * fabs(x));
    double poly = k * (0.319381530 + k * (-0.356563782 + k * (1.781477937 + k * (-1.821255978 + k * 1.330274429))));
    double w = 1.0 - 0.3989422804014327 * exp(-0.5 * x * x) * poly;
    return x < 0.0 ? 1.0 - w : w;
}

static int64_t cents(double x) { return (int64_t)round(x * 100.0); }

int main(void) {
    const int64_t n = 20000000;
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) {
        double s = 50.0 + 100.0 * unit(5 * i), k = 50.0 + 100.0 * unit(5 * i + 1);
        double t = 0.1 + 2.0 * unit(5 * i + 2), r = 0.01 + 0.05 * unit(5 * i + 3), v = 0.1 + 0.5 * unit(5 * i + 4);
        double root = v * sqrt(t);
        double d1 = (log(s / k) + (r + 0.5 * v * v) * t) / root, d2 = d1 - root;
        double far = k * exp(-r * t);
        total += cents(s * cnd(d1) - far * cnd(d2)) + cents(far * cnd(-d2) - s * cnd(-d1));
    }
    printf("%lld %lld\n", (long long)n, (long long)total);
    return 0;
}
