// Generates arithmetic expressions of numbers, names and the operators
// + - * / (sometimes with spaces around an operator), splits each one into
// tokens and prints how many numbers, names and operators there were.
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

static char *make_expr(void) {
    char buf[1024];
    int len = 0;
    int64_t k = next() % 8 + 2;
    for (int i = 0; i < k; i++) {
        if (i > 0) {
            char op = "+-*/"[next() % 4];
            if (next() % 3 == 0) len += sprintf(buf + len, " %c ", op);
            else buf[len++] = op;
        }
        if (next() % 3 != 0) {
            len += sprintf(buf + len, "%lld", (long long)(next() % 1000));
        } else {
            int64_t n = next() % 5 + 1;
            for (int j = 0; j < n; j++) buf[len++] = (char)('a' + next() % 26);
        }
    }
    buf[len] = 0;
    char *s = malloc(len + 1);
    memcpy(s, buf, len + 1);
    return s;
}

static int is_digit(char c) { return c >= '0' && c <= '9'; }
static int is_letter(char c) { return c >= 'a' && c <= 'z'; }

int main(void) {
    char **exprs = malloc(EXPRS * sizeof *exprs);
    for (int e = 0; e < EXPRS; e++) exprs[e] = make_expr();
    int64_t numbers = 0, names = 0, operators = 0;
    for (int e = 0; e < EXPRS; e++) {
        const char *s = exprs[e];
        int i = 0, n = (int)strlen(s);
        while (i < n) {
            if (is_digit(s[i])) {
                while (i < n && is_digit(s[i])) i++;
                numbers++;
            } else if (is_letter(s[i])) {
                while (i < n && is_letter(s[i])) i++;
                names++;
            } else if (s[i] == ' ') {
                i++;
            } else {
                operators++;
                i++;
            }
        }
    }
    printf("numbers %lld\n", (long long)numbers);
    printf("names %lld\n", (long long)names);
    printf("operators %lld\n", (long long)operators);
    for (int e = 0; e < EXPRS; e++) free(exprs[e]);
    free(exprs);
    return 0;
}
