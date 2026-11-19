// hull: the convex hull of 2,000,000 points with integer coordinates in a
// disc of radius 10^6 (from splitmix64), by Andrew's monotone chain:
// sorted by x then y, then the lower and upper chains as stacks. Prints
// the number of hull points, twice the hull's area, and the sum of the
// hull's x coordinates.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 2000000
#define R 1000000

typedef struct { int64_t x, y; } pt;

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t floordiv2(int64_t v) { return v >= 0 ? v / 2 : -((-v + 1) / 2); }

static int cmp(const void *a, const void *b) {
    const pt *p = a, *q = b;
    if (p->x != q->x) return p->x < q->x ? -1 : 1;
    return (p->y > q->y) - (p->y < q->y);
}

static int64_t cross(pt o, pt a, pt b) { return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x); }

// one chain over ps in the given direction, into h; its length (the last
// point left off)
static int64_t chain(const pt *ps, int64_t n, int dir, pt *h) {
    int64_t len = 0;
    for (int64_t k = 0; k < n; k++) {
        pt p = ps[dir > 0 ? k : n - 1 - k];
        while (len >= 2 && cross(h[len - 2], h[len - 1], p) <= 0) len--;
        h[len++] = p;
    }
    return len - 1;
}

int main(void) {
    pt *ps = malloc(sizeof(pt) * N), *h = malloc(sizeof(pt) * 2 * N);
    for (int64_t k = 0; k < N; k++) {
        int64_t x = mix(2 * k) % (2 * R + 1) - R, y = mix(2 * k + 1) % (2 * R + 1) - R;
        if (x * x + y * y > (int64_t)R * R) {
            x = floordiv2(x);
            y = floordiv2(y);
        }
        ps[k] = (pt){x, y};
    }
    qsort(ps, N, sizeof(pt), cmp);
    int64_t m = chain(ps, N, 1, h);
    m += chain(ps, N, -1, h + m);
    int64_t area2 = 0, sx = 0;
    for (int64_t i = 0; i < m; i++) {
        pt a = h[i], b = h[(i + 1) % m];
        area2 += a.x * b.y - b.x * a.y;
        sx += a.x;
    }
    printf("%lld %lld %lld\n", (long long)m, (long long)area2, (long long)sx);
    return 0;
}
