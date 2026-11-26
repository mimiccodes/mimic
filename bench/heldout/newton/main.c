// Square roots of generated numbers by Newton's method, iterated until
// |g*g - v| < 1e-12 * v, and summed.
#include <stdio.h>
#include <stdint.h>
#include <math.h>

static double newton_sqrt(double v) {
    double g = v;
    while (fabs(g * g - v) >= 1e-12 * v) {
        g = (g + v / g) / 2.0;
    }
    return g;
}

int main(void) {
    const int64_t n = 3000000;
    int64_t x = 42;
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        double v = (double)(x % 1000000 + 1);
        total += newton_sqrt(v);
    }
    printf("%.2f\n", total);
    return 0;
}
