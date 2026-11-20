// kmp: every occurrence, overlapping ones too, of 8 patterns in 4 MiB of
// generated text, by Knuth-Morris-Pratt. Prints each pattern's count.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int64_t count(const uint8_t *t, int64_t n, const char *pat) {
    int m = (int)strlen(pat), fail[64];
    const uint8_t *p = (const uint8_t *)pat;
    fail[0] = 0;
    for (int i = 1, k = 0; i < m; i++) {
        while (k > 0 && p[k] != p[i]) k = fail[k - 1];
        if (p[k] == p[i]) k++;
        fail[i] = k;
    }
    int64_t c = 0;
    for (int64_t i = 0, k = 0; i < n; i++) {
        while (k > 0 && p[k] != t[i]) k = fail[k - 1];
        if (p[k] == t[i]) k++;
        if (k == m) {
            c++;
            k = fail[k - 1];
        }
    }
    return c;
}

int main(void) {
    const int64_t n = 1 << 22;
    static const char *words[20] = {"the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector", "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"};
    uint8_t *t = malloc(n + 32);
    int64_t len = 0;
    for (int64_t i = 0; len < n; i++) {
        const char *w = words[i % 7 ? (i * 2654435761LL >> 13) % 20 : (i * 40503 >> 7) % 20];
        size_t k = strlen(w);
        memcpy(t + len, w, k);
        len += k;
        if (i % 11 == 10) t[len++] = '.';
        t[len++] = ' ';
    }
    static const char *pats[8] = {"the", "parallel array", "core loop", "a", "zebra quartz", "e the", "mimic", "of the of"};
    for (int i = 0; i < 8; i++) printf(i ? " %lld" : "%lld", (long long)count(t, n, pats[i]));
    printf("\n");
    free(t);
    return 0;
}
