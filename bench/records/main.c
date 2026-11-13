/* records: a million records with a name and numbers, updated five times
 * (in place), then summed (C, Rust and Go update in place;
 * Mimic builds new records: see BENCHMARK.md) */
#include <stdio.h>
#include <stdlib.h>

typedef struct { long id; const char *name; double x, v; } P;

int main(void) {
    long n = 1000000;
    P *ps = malloc(sizeof(P) * n);
    for (long i = 0; i < n; i++) ps[i] = (P){i, "p", (double)i, 1.0};
    for (int s = 0; s < 5; s++)
        for (long i = 0; i < n; i++) {
            ps[i].x += ps[i].v;
            ps[i].v *= 0.5;
        }
    double sx = 0, se = 0;
    for (long i = 0; i < n; i++) sx += ps[i].x;
    for (long i = 0; i < n; i++) se += ps[i].v * ps[i].v * 1024.0;
    printf("%.1f\n%.1f\n", sx, se);
    free(ps);
    return 0;
}
