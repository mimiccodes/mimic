// regex: a Thompson NFA engine. Four patterns are parsed, compiled into an
// automaton of at most 63 states and run over 50,000 generated strings
// each, the live states kept as the bits of one integer. Prints how many
// strings each pattern matches in full.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { CH, CAT, ALT, STAR, PLUS, OPT };
typedef struct Re { int k, lo, hi; struct Re *a, *b; } Re;
static Re pool[256];
static int npool;
static Re *mk(int k, int lo, int hi, Re *a, Re *b) { Re *r = &pool[npool++]; r->k = k; r->lo = lo; r->hi = hi; r->a = a; r->b = b; return r; }

static const char *p;
static int pi_, plen;
static Re *alt(void);
static Re *atom(void) {
    if (p[pi_] == '(') { pi_++; Re *a = alt(); pi_++; return a; }
    if (p[pi_] == '[') { Re *r = mk(CH, p[pi_ + 1], p[pi_ + 3], 0, 0); pi_ += 5; return r; }
    Re *r = mk(CH, p[pi_], p[pi_], 0, 0);
    pi_++;
    return r;
}
static Re *rep(void) {
    Re *a = atom();
    while (pi_ < plen && (p[pi_] == '*' || p[pi_] == '+' || p[pi_] == '?')) {
        a = mk(p[pi_] == '*' ? STAR : p[pi_] == '+' ? PLUS : OPT, 0, 0, a, 0);
        pi_++;
    }
    return a;
}
static Re *cat(void) {
    Re *a = rep();
    if (pi_ < plen && p[pi_] != '|' && p[pi_] != ')') return mk(CAT, 0, 0, a, cat());
    return a;
}
static Re *alt(void) {
    Re *a = cat();
    if (pi_ < plen && p[pi_] == '|') { pi_++; return mk(ALT, 0, 0, a, alt()); }
    return a;
}

static int kind[64], lo[64], hi[64], o1[64], o2[64], nn;
static int add(int k, int l, int h, int a, int b) { kind[nn] = k; lo[nn] = l; hi[nn] = h; o1[nn] = a; o2[nn] = b; return nn++; }
static int comp(Re *r, int next) {
    switch (r->k) {
    case CH: return add(0, r->lo, r->hi, next, -1);
    case CAT: return comp(r->a, comp(r->b, next));
    case ALT: { int sa = comp(r->a, next), sb = comp(r->b, next); return add(1, 0, 0, sa, sb); }
    case STAR: { int s = add(1, 0, 0, -1, next); o1[s] = comp(r->a, s); return s; }
    case PLUS: { int s = add(1, 0, 0, -1, next); int body = comp(r->a, s); o1[s] = body; return body; }
    default: { int sa = comp(r->a, next); return add(1, 0, 0, sa, next); }
    }
}
static uint64_t closure(int s, uint64_t seen) {
    if (seen >> s & 1) return 0;
    if (kind[s] == 1) return closure(o1[s], seen | 1ull << s) | closure(o2[s], seen | 1ull << s);
    return 1ull << s;
}
static int64_t mix(int64_t v, int64_t m) { return (v * 2654435761LL >> 11) % m; }

static int count(int pn, const char *pattern, const char *alpha) {
    p = pattern; pi_ = 0; plen = (int)strlen(pattern); npool = 0; nn = 0;
    Re *ast = alt();
    int final = add(2, 0, 0, -1, -1), start = comp(ast, final);
    uint64_t cl[64];
    for (int s = 0; s < nn; s++) cl[s] = closure(s, 0);
    int na = (int)strlen(alpha), c = 0;
    char text[64];
    for (int64_t j = 0; j < 50000; j++) {
        int n = 8 + (int)mix(pn * 7919 + j, 33);
        for (int k = 0; k < n; k++) text[k] = alpha[mix(pn * 1000003LL + j * 131 + k, na)];
        uint64_t live = cl[start];
        int at = 0;
        for (; at < n && live; at++) {
            uint64_t next = 0;
            for (int s = 0; s < nn; s++)
                if ((live >> s & 1) && kind[s] == 0 && lo[s] <= text[at] && text[at] <= hi[s]) next |= cl[o1[s]];
            live = next;
        }
        c += at == n && (live >> final & 1);
    }
    return c;
}

int main(void) {
    int a = count(0, "(a|b)*abb", "ab"), b = count(1, "(0|1(01*0)*1)*", "01");
    int c = count(2, "[a-c]*b[a-c]?c+", "abc"), d = count(3, "(x|y|z)*(xy|yz)+z?", "xyz");
    printf("%d %d %d %d\n", a, b, c, d);
    return 0;
}
