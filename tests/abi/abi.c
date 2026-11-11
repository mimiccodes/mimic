#include "abi.h"
struct tiny tiny_echo(struct tiny t) { t.c += 1; return t; }
struct pair_f pair_swap(struct pair_f p) { struct pair_f r = { p.b, p.a }; return r; }
double quad_sum(struct quad_d q) { return q.a + q.b + q.c + q.d; }
struct quad_d quad_scale(struct quad_d q, double k) { struct quad_d r = { q.a * k, q.b * k, q.c * k, q.d * k }; return r; }
struct mixed mixed_bump(struct mixed m) { m.i += 1; m.d += 0.5; return m; }
long long big_sum(struct big b) { return b.a + b.b + b.c + b.d + b.e + b.f + b.g + b.h; }
struct big big_make(long long base) { struct big r = { base, base + 1, base + 2, base + 3, base + 4, base + 5, base + 6, base + 7 }; return r; }
struct odd odd_echo(struct odd o) { o.a += 1; o.b += 2; o.c += 3; o.d += 4; return o; }
struct nest nest_make(float a, float b, char c, double x) { struct nest n = { { a, b }, { c }, x }; return n; }
double many(struct pair_f a, struct pair_f b, struct pair_f c, struct pair_f d, struct pair_f e,
            struct mixed f, struct mixed g, struct mixed h, double i, long long j) {
    return a.a + b.b + c.a + d.b + e.a + f.i + f.d + g.i + g.d + h.i + h.d + i + (double)j;
}
