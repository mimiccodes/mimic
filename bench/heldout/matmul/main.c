// Multiply two n x n matrices of floats (rows of generated whole numbers)
// and print the trace and the sum of all entries of the product.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

#define N 400

static int64_t x = 42;

static int64_t next(void) {
    x = (x * 1103515245 + 12345) % 2147483648;
    return x;
}

int main(void) {
    double (*a)[N] = malloc(sizeof(double[N][N]));
    double (*b)[N] = malloc(sizeof(double[N][N]));
    double (*c)[N] = malloc(sizeof(double[N][N]));
    for (int i = 0; i < N; i++)
        for (int j = 0; j < N; j++)
            a[i][j] = (double)(next() % 100);
    for (int i = 0; i < N; i++)
        for (int j = 0; j < N; j++)
            b[i][j] = (double)(next() % 100);

    for (int i = 0; i < N; i++) {
        for (int j = 0; j < N; j++) {
            double s = 0.0;
            for (int k = 0; k < N; k++) s += a[i][k] * b[k][j];
            c[i][j] = s;
        }
    }

    double trace = 0.0, total = 0.0;
    for (int i = 0; i < N; i++) trace += c[i][i];
    for (int i = 0; i < N; i++)
        for (int j = 0; j < N; j++) total += c[i][j];
    printf("%.6f\n%.6f\n", trace, total);
    free(a);
    free(b);
    free(c);
    return 0;
}
