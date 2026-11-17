// The machine's memory bandwidth on one core: the STREAM triad
// (a = b + s × c) over arrays far larger than the caches. `stream <n>
// <rounds>` prints a checksum; bench/run.py times it (twice, to take the
// start-up and first touch away) and counts 24 bytes per element per round
// (two read, one written), as STREAM does.
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 1L << 24, rounds = argc > 2 ? atol(argv[2]) : 10;
    double *a = malloc(sizeof(double) * n), *b = malloc(sizeof(double) * n), *c = malloc(sizeof(double) * n);
    if (!a || !b || !c) return 1;
    for (long i = 0; i < n; i++) {
        a[i] = 0;
        b[i] = (double)(i % 7);
        c[i] = (double)(i % 5);
    }
    const double s = 3.0;
    for (long r = 0; r < rounds; r++) {
        for (long i = 0; i < n; i++) a[i] = b[i] + s * c[i];
        b[r % n] += a[(r * 7) % n]; // the next round depends on this one
    }
    printf("%.1f\n", a[n / 2] + a[n - 1] + b[0]);
    free(a);
    free(b);
    free(c);
    return 0;
}
