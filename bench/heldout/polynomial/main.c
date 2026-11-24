// Multiplies 2,000 pairs of generated polynomials of degree 300 (integer
// coefficients from -100 to 100) and prints a checksum of all the product
// coefficients.
#include <stdint.h>
#include <stdio.h>

#define PAIRS 2000
#define DEGREE 300

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

int main(void) {
    int64_t a[DEGREE + 1], b[DEGREE + 1], c[2 * DEGREE + 1];
    int64_t checksum = 0;
    for (int p = 0; p < PAIRS; p++) {
        for (int i = 0; i <= DEGREE; i++) a[i] = next() % 201 - 100;
        for (int i = 0; i <= DEGREE; i++) b[i] = next() % 201 - 100;
        for (int k = 0; k <= 2 * DEGREE; k++) c[k] = 0;
        for (int i = 0; i <= DEGREE; i++)
            for (int j = 0; j <= DEGREE; j++) c[i + j] += a[i] * b[j];
        for (int k = 0; k <= 2 * DEGREE; k++) checksum += c[k] * (k + 1);
    }
    printf("%lld\n", (long long)checksum);
    return 0;
}
