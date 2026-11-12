#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const int n = 1024;
    double *a = malloc(sizeof(double) * n * n), *b = malloc(sizeof(double) * n * n), *c = calloc((size_t)n * n, sizeof(double));
    for (int i = 0; i < n; i++)
        for (int j = 0; j < n; j++) {
            a[i * n + j] = (double)((i * j) % 7);
            b[i * n + j] = (double)((i + 2 * j) % 5);
        }
    for (int i = 0; i < n; i++)
        for (int k = 0; k < n; k++) {
            double x = a[i * n + k];
            for (int j = 0; j < n; j++) c[i * n + j] += x * b[k * n + j];
        }
    double s = 0;
    for (int i = 0; i < n * n; i++) s += c[i];
    printf("%.1f\n", s);
    return 0;
}
