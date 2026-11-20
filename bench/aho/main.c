// aho: Aho-Corasick over 4 MiB of generated text with 45 patterns: the
// trie, failure links breadth-first folded into a full transition table,
// then one pass. The text is cut into 64 pieces scanned one after another
// (each starting the longest pattern's length early), as the Mimic version
// cuts it. Prints the matches of all patterns and the positions where at
// least one ends.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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
    char pats[45][32];
    int np = 0;
    for (int i = 0; i < 20; i++) strcpy(pats[np++], words[i]);
    for (int i = 0; i < 20; i++) snprintf(pats[np++], 32, "%s %s", words[i], words[(i * 7 + 3) % 20]);
    static const char *extra[5] = {"he", "ar", "or", "e t", "ll"};
    for (int i = 0; i < 5; i++) strcpy(pats[np++], extra[i]);

    int size = 1, longest = 0;
    for (int i = 0; i < np; i++) {
        int l = (int)strlen(pats[i]);
        size += l;
        if (l > longest) longest = l;
    }
    int *go = malloc(sizeof(int) * size * 256), *fail = calloc(size, sizeof(int)), *queue = malloc(sizeof(int) * size);
    int64_t *out = calloc(size, sizeof(int64_t));
    for (int i = 0; i < size * 256; i++) go[i] = -1;
    int states = 1;
    for (int p = 0; p < np; p++) {
        int cur = 0;
        for (const uint8_t *c = (const uint8_t *)pats[p]; *c; c++) {
            if (go[cur * 256 + *c] < 0) go[cur * 256 + *c] = states++;
            cur = go[cur * 256 + *c];
        }
        out[cur]++;
    }
    int head = 0, tail = 1;
    queue[0] = 0;
    while (head < tail) {
        int r = queue[head++];
        if (r) out[r] += out[fail[r]];
        for (int c = 0; c < 256; c++) {
            int u = go[r * 256 + c], back = r ? go[fail[r] * 256 + c] : 0;
            if (u >= 0) {
                fail[u] = back;
                queue[tail++] = u;
            } else go[r * 256 + c] = back;
        }
    }
    int64_t total = 0, ends = 0, cut = n / 64;
    for (int64_t j = 0; j < 64; j++) {
        int64_t lo = j * cut - longest + 1, start = j * cut;
        if (lo < 0) lo = 0;
        int s = 0;
        for (int64_t i = lo; i < (j + 1) * cut; i++) {
            s = go[s * 256 + t[i]];
            if (i >= start) {
                total += out[s];
                ends += out[s] > 0;
            }
        }
    }
    printf("%lld %lld\n", (long long)total, (long long)ends);
    free(t); free(go); free(fail); free(queue); free(out);
    return 0;
}
