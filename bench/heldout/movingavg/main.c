// A 50-wide moving average over 10,000,000 generated floats (multiples of
// 1/64, so every sum is exact); prints the largest average and the last one.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 10000000
#define WIDTH 50

int main(void) {
    double *v = malloc(N * sizeof *v);
    int64_t x = 42;
    for (int i = 0; i < N; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        v[i] = (double)(x % 10000) / 64.0;
    }
    double sum = 0.0, best = 0.0, last = 0.0;
    for (int i = 0; i < N; i++) {
        sum += v[i];
        if (i >= WIDTH) sum -= v[i - WIDTH];
        if (i >= WIDTH - 1) {
            last = sum / WIDTH;
            if (i == WIDTH - 1 || last > best) best = last;
        }
    }
    printf("%.6f\n", best);
    printf("%.6f\n", last);
    free(v);
    return 0;
}
