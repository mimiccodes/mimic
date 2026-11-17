// The machine's peak floating-point rate on one core: independent fused
// multiply-adds on this CPU's widest registers, more than it can have in
// flight, so only its arithmetic units limit it. `peak <rounds>` prints a
// checksum; bench/run.py times it (twice, to take the start-up away) and
// counts 2 operations per lane per multiply-add (`peak w` prints them per
// round).
//
// A probe of the hardware, not a benchmark: each multiply-add is the
// CPU's own instruction (inline assembly), because a compiler rightly
// turns a multiply-add whose product never changes into an add.
#include <stdio.h>
#include <stdlib.h>

#if defined(__aarch64__)
#define LANES 2
#define ACC 24 // of 32 registers; the other two are the factors
#define FMA(acc, x, y) __asm__ volatile("fmla %0.2d, %1.2d, %2.2d" : "+w"(acc) : "w"(x), "w"(y))
#elif defined(__AVX512F__)
#define LANES 8
#define ACC 24
#define FMA(acc, x, y) __asm__ volatile("vfmadd231pd %2, %1, %0" : "+v"(acc) : "v"(x), "v"(y))
#elif defined(__FMA__)
#define LANES 4
#define ACC 12 // of 16 registers
#define FMA(acc, x, y) __asm__ volatile("vfmadd231pd %2, %1, %0" : "+x"(acc) : "x"(x), "x"(y))
#else
// no fused multiply-add instruction: a multiply and an add, counted as two
#define LANES 2
#define ACC 12
#define FMA(acc, x, y) (acc += x * y)
#endif
typedef double vec __attribute__((vector_size(LANES * 8)));

int main(int argc, char **argv) {
    if (argc > 1 && argv[1][0] == 'w') { // the work per round, for run.py
        printf("%d\n", ACC * LANES * 2);
        return 0;
    }
    long rounds = argc > 1 ? atol(argv[1]) : 1000000;
    vec acc[ACC], x, y;
    for (int i = 0; i < LANES; i++) {
        x[i] = 1e-9 * (i + 1);
        y[i] = 0.5;
    }
    for (int k = 0; k < ACC; k++) acc[k] = x * (double)(k + 1);
    for (long r = 0; r < rounds; r++)
        for (int k = 0; k < ACC; k++) FMA(acc[k], x, y);
    double s = 0;
    for (int k = 0; k < ACC; k++)
        for (int i = 0; i < LANES; i++) s += acc[k][i];
    printf("%.6f\n", s);
    return 0;
}
