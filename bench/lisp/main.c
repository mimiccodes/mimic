// lisp: 50,000 random s-expressions (from splitmix64) written as text, then
// tokenised and evaluated by a recursive evaluator, all arithmetic modulo
// 1,000,000,007. Prints the number of tokens, the sum of the values modulo
// p, and the largest value.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define P 1000000007LL

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static const char *OPS[5] = {"+", "-", "*", "max", "min"};

static char *gen(char *p, int64_t s, int d) {
    if (d == 0 || mix(s) % 3 == 0) return p + sprintf(p, "%lld", (long long)(mix(s + 1) % 100));
    p += sprintf(p, "(%s ", OPS[mix(s + 2) % 5]);
    p = gen(p, mix(s + 3), d - 1);
    *p++ = ' ';
    p = gen(p, mix(s + 4), d - 1);
    *p++ = ')';
    return p;
}

static char **toks;
static int64_t ntoks;

static int64_t eval(int64_t *i) {
    char *t = toks[*i];
    if (t[0] != '(') {
        (*i)++;
        return strtoll(t, NULL, 10);
    }
    char *op = toks[*i + 1];
    *i += 2;
    int64_t a = eval(i), b = eval(i), v;
    if (!strcmp(op, "+")) v = (a + b) % P;
    else if (!strcmp(op, "-")) v = (a - b + P) % P;
    else if (!strcmp(op, "*")) v = a * b % P;
    else if (!strcmp(op, "max")) v = a > b ? a : b;
    else v = a < b ? a : b;
    (*i)++;
    return v;
}

int main(void) {
    char *text = malloc(64 << 20), *p = text;
    for (int64_t k = 0; k < 50000; k++) {
        if (k) *p++ = '\n';
        p = gen(p, k, 7);
    }
    *p = 0;
    // spaced parentheses, then split on white space
    char *spaced = malloc(3 * (size_t)(p - text) + 1), *q = spaced;
    for (char *c = text; *c; c++) {
        if (*c == '(' || *c == ')') {
            *q++ = ' ';
            *q++ = *c;
            *q++ = ' ';
        } else *q++ = *c;
    }
    *q = 0;
    toks = malloc(sizeof(char *) * 8000000);
    for (char *t = strtok(spaced, " \n"); t; t = strtok(NULL, " \n")) toks[ntoks++] = t;
    int64_t i = 0, sum = 0, most = 0;
    while (i < ntoks) {
        int64_t v = eval(&i);
        sum = (sum + v) % P;
        if (v > most) most = v;
    }
    printf("%lld %lld %lld\n", (long long)ntoks, (long long)sum, (long long)most);
    return 0;
}
