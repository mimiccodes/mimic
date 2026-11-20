// expr: 300,000 arithmetic expressions (four shapes, with parentheses and
// precedence) written out as text, then tokenized, parsed by precedence
// climbing and evaluated; prints the sum of their values.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int64_t num(int64_t i, int64_t k) { return (i * k + k * 31) % 97 + 1; }

static const char **tok;
static int ntok;

static int prec(const char *op) { return *op == '+' || *op == '-' ? 1 : *op == '*' ? 2 : 0; }

static int64_t expr(int *p, int least);

static int64_t atom(int *p) {
    if (tok[*p][0] == '(') {
        (*p)++;
        int64_t v = expr(p, 1);
        (*p)++;
        return v;
    }
    return strtoll(tok[(*p)++], NULL, 10);
}

static int64_t expr(int *p, int least) {
    int64_t v = atom(p);
    while (*p < ntok && prec(tok[*p]) >= least) {
        char op = tok[*p][0];
        (*p)++;
        int64_t r = expr(p, prec(&op) + 1);
        v = op == '+' ? v + r : op == '-' ? v - r : v * r;
    }
    return v;
}

int main(void) {
    const int64_t n = 300000;
    char buf[128];
    const char *words[32];
    int64_t sum = 0;
    for (int64_t i = 0; i < n; i++) {
        long long a = num(i, 3), b = num(i, 7), c = num(i, 11), d = num(i, 13), e = num(i, 17), f = num(i, 19);
        switch (i % 4) {
        case 0: snprintf(buf, sizeof buf, "%lld + %lld * %lld - %lld", a, b, c, d); break;
        case 1: snprintf(buf, sizeof buf, "( %lld + %lld ) * ( %lld - %lld ) + %lld", a, b, c, d, e); break;
        case 2: snprintf(buf, sizeof buf, "%lld * ( %lld + %lld * ( %lld - %lld ) ) - %lld", a, b, c, d, e, f); break;
        default: snprintf(buf, sizeof buf, "( ( %lld - %lld ) * %lld + %lld ) * ( %lld + %lld )", a, b, c, d, e, f);
        }
        // tokens: the words between spaces
        ntok = 0;
        for (char *s = strtok(buf, " "); s; s = strtok(NULL, " ")) words[ntok++] = s;
        tok = words;
        int p = 0;
        sum += expr(&p, 1);
    }
    printf("%lld\n", (long long)sum);
    return 0;
}
