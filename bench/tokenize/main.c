// tokenize: 200,000 lines of generated C-like source scanned into tokens
// (identifiers, numbers, strings, operators and punctuation); prints how
// many of each and the total length of the identifiers.
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    const int64_t n = 200000;
    char b[128];
    int64_t ids = 0, nums = 0, strs = 0, ops = 0, len = 0;
    for (int64_t i = 0; i < n; i++) {
        int m;
        switch (i % 3) {
        case 0: m = snprintf(b, sizeof b, "let x%lld = foo(y%lld, %lld) + \"s%lld\" * 3;", (long long)i, (long long)(i % 97), (long long)(i * 7 % 1000), (long long)(i % 13)); break;
        case 1: m = snprintf(b, sizeof b, "if (count_%lld >= %lld) { total += weight[%lld]; }", (long long)(i % 50), (long long)(i % 9), (long long)(i % 31)); break;
        default: m = snprintf(b, sizeof b, "return bar_%lld(\"a b\", z%lld) - %lld / 2;", (long long)(i % 7), (long long)i, (long long)(i % 100));
        }
        for (int p = 0; p < m;) {
            unsigned char c = b[p];
            if (c == ' ') {
                p++;
            } else if (isalpha(c) || c == '_') {
                int s = p++;
                while (p < m && (isalnum((unsigned char)b[p]) || b[p] == '_')) p++;
                ids++;
                len += p - s;
            } else if (isdigit(c)) {
                while (p < m && isdigit((unsigned char)b[p])) p++;
                nums++;
            } else if (c == '"') {
                p = (int)((char *)memchr(b + p + 1, '"', m - p - 1) - b) + 1;
                strs++;
            } else if ((c == '=' || c == '>' || c == '+') && p + 1 < m && b[p + 1] == '=') {
                p += 2;
                ops++;
            } else {
                p++;
                ops++;
            }
        }
    }
    printf("%lld %lld %lld %lld %lld\n", (long long)ids, (long long)nums, (long long)strs, (long long)ops, (long long)len);
    return 0;
}
