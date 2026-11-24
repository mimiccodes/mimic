// Generates arithmetic expressions with + - * / and parentheses and
// evaluates them by recursive descent (integer division truncates toward
// zero); prints the sum of the results.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define EXPRS 200000

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static char buf[4096];
static int blen;

static void gen_expr(int depth);

static void gen_factor(int depth) {
    if (depth > 0 && next() % 4 == 0) {
        buf[blen++] = '(';
        gen_expr(depth - 1);
        buf[blen++] = ')';
    } else {
        blen += sprintf(buf + blen, "%lld", (long long)(next() % 20));
    }
}

static void gen_term(int depth) {
    gen_factor(depth);
    int64_t c = next() % 3;
    if (c == 0) {
        buf[blen++] = '/';
        blen += sprintf(buf + blen, "%lld", (long long)(next() % 19 + 1));
    } else if (c == 1) {
        buf[blen++] = '*';
        gen_factor(depth);
    }
}

static void gen_expr(int depth) {
    int64_t terms = next() % 3 + 1;
    gen_term(depth);
    for (int t = 1; t < terms; t++) {
        buf[blen++] = (next() % 3 == 2) ? '-' : '+';
        gen_term(depth);
    }
}

static const char *p;

static int64_t parse_expr(void);

static int64_t parse_factor(void) {
    if (*p == '(') {
        p++;
        int64_t v = parse_expr();
        p++;  // ')'
        return v;
    }
    int64_t v = 0;
    while (*p >= '0' && *p <= '9') v = v * 10 + (*p++ - '0');
    return v;
}

static int64_t parse_term(void) {
    int64_t v = parse_factor();
    while (*p == '*' || *p == '/') {
        char op = *p++;
        int64_t r = parse_factor();
        v = (op == '*') ? v * r : v / r;
    }
    return v;
}

static int64_t parse_expr(void) {
    int64_t v = parse_term();
    while (*p == '+' || *p == '-') {
        char op = *p++;
        int64_t r = parse_term();
        v = (op == '+') ? v + r : v - r;
    }
    return v;
}

int main(void) {
    char **exprs = malloc(EXPRS * sizeof *exprs);
    for (int e = 0; e < EXPRS; e++) {
        blen = 0;
        gen_expr(2);
        buf[blen] = 0;
        exprs[e] = malloc(blen + 1);
        memcpy(exprs[e], buf, blen + 1);
    }
    int64_t total = 0;
    for (int e = 0; e < EXPRS; e++) {
        p = exprs[e];
        total += parse_expr();
    }
    printf("%lld\n", (long long)total);
    for (int e = 0; e < EXPRS; e++) free(exprs[e]);
    free(exprs);
    return 0;
}
