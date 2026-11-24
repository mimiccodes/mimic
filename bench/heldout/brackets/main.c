// Generate strings of brackets ()[]{}, check each for balance with a
// stack, and print how many are balanced and the deepest nesting seen.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

static const char *FRAGMENTS[11] = {
    "()", "[]", "{}", "([])", "{()}", "[{}]", "(()())", "{[()]}", "[[]]", "(", "]"};

static int64_t x = 42;

static int64_t next(void) {
    x = (x * 1103515245 + 12345) % 2147483648;
    return x;
}

static bool matches(char open, char close) {
    return (open == '(' && close == ')') || (open == '[' && close == ']') || (open == '{' && close == '}');
}

// returns whether s is balanced; *deepest is the largest stack size reached
static bool check(const char *s, int64_t *deepest) {
    char stack[256];
    int64_t top = 0;
    *deepest = 0;
    for (const char *p = s; *p; p++) {
        char c = *p;
        if (c == '(' || c == '[' || c == '{') {
            stack[top++] = c;
            if (top > *deepest) *deepest = top;
        } else {
            if (top == 0 || !matches(stack[top - 1], c)) return false;
            top--;
        }
    }
    return top == 0;
}

int main(void) {
    const int64_t count = 200000;
    char s[256];
    int64_t balanced = 0, deepest = 0;
    for (int64_t i = 0; i < count; i++) {
        int64_t pieces = 1 + next() % 13;
        int64_t len = 0;
        char body[200];
        for (int64_t k = 0; k < pieces; k++) {
            const char *f = FRAGMENTS[next() % 11];
            size_t fl = strlen(f);
            memcpy(body + len, f, fl);
            len += (int64_t)fl;
        }
        int64_t wrap = next() % 5;
        int64_t n = 0;
        for (int64_t k = 0; k < wrap; k++) s[n++] = '(';
        memcpy(s + n, body, (size_t)len);
        n += len;
        for (int64_t k = 0; k < wrap; k++) s[n++] = ')';
        s[n] = '\0';

        int64_t depth;
        if (check(s, &depth)) balanced++;
        if (depth > deepest) deepest = depth;
    }
    printf("%lld\n%lld\n", (long long)balanced, (long long)deepest);
    return 0;
}
