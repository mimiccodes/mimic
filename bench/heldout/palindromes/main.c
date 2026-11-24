// Count the palindromic substrings of a generated string over "ab" by
// expanding around each centre.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

static int64_t expand(const char *s, int64_t n, int64_t l, int64_t r) {
    int64_t found = 0;
    while (l >= 0 && r < n && s[l] == s[r]) {
        found++;
        l--;
        r++;
    }
    return found;
}

int main(void) {
    const int64_t n = 10000000;
    int64_t x = 42;
    char *s = malloc((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        s[i] = "ab"[(x / 65536) % 2];
    }
    s[n] = '\0';

    int64_t total = 0;
    for (int64_t c = 0; c < n; c++) {
        total += expand(s, n, c, c);      // odd lengths
        total += expand(s, n, c, c + 1);  // even lengths
    }
    printf("%lld\n", (long long)total);
    free(s);
    return 0;
}
