// 2,000 particles move in a 100 x 100 box with reflecting walls for 10,000
// steps; prints the sum of the x and y positions. Positions, velocities and
// the time step are multiples of powers of two, so every sum is exact.
#include <stdint.h>
#include <stdio.h>

#define COUNT 2000
#define STEPS 10000

typedef struct {
    double x, y, vx, vy;
} Particle;

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static void bounce(double *p, double *v) {
    if (*p < 0.0) {
        *p = -*p;
        *v = -*v;
    } else if (*p > 100.0) {
        *p = 200.0 - *p;
        *v = -*v;
    }
}

int main(void) {
    static Particle ps[COUNT];
    for (int i = 0; i < COUNT; i++) {
        ps[i].x = (double)(next() % 1600) / 16.0;
        ps[i].y = (double)(next() % 1600) / 16.0;
        ps[i].vx = (double)(next() % 2001 - 1000) / 256.0;
        ps[i].vy = (double)(next() % 2001 - 1000) / 256.0;
    }
    const double dt = 0.125;
    for (int s = 0; s < STEPS; s++) {
        for (int i = 0; i < COUNT; i++) {
            Particle *p = &ps[i];
            p->x = p->x + p->vx * dt;
            p->y = p->y + p->vy * dt;
            bounce(&p->x, &p->vx);
            bounce(&p->y, &p->vy);
        }
    }
    double sx = 0.0, sy = 0.0;
    for (int i = 0; i < COUNT; i++) {
        sx += ps[i].x;
        sy += ps[i].y;
    }
    printf("%.6f\n", sx);
    printf("%.6f\n", sy);
    return 0;
}
