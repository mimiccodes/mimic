// lz77: 512 KiB of generated text compressed with greedy LZ77 (a hash table
// of 3-byte prefixes, matches of 4 to 255 bytes up to 64 KiB back), then
// decompressed and compared. Prints the literals, the matches, the bytes
// covered by matches, and 1 when the round trip is exact.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const int64_t n = 1 << 19;
    static const char *words[16] = {"the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "array", "parallel", "mimic", "kernel", "fusion", "vector", "memory", "cache"};
    uint8_t *d = malloc(n + 16);
    int64_t len = 0;
    for (int64_t i = 0; len < n; i++) {
        const char *w = words[(i * 2654435761LL >> 13) % 16];
        size_t k = strlen(w);
        memcpy(d + len, w, k);
        d[len + k] = ' ';
        len += k + 1;
    }
    uint32_t *table = calloc(65536, sizeof *table);
    int64_t *toks = malloc(sizeof(int64_t) * 2 * n), t = 0, lits = 0, matches = 0, covered = 0;
    for (int64_t p = 0; p < n;) {
        if (p + 3 <= n) {
            uint32_t h = (uint32_t)((((int64_t)d[p] << 16 | d[p + 1] << 8 | d[p + 2]) * 2654435761LL >> 15) & 65535);
            int64_t c = (int64_t)table[h] - 1;
            table[h] = (uint32_t)(p + 1);
            if (c >= 0 && p - c <= 65535) {
                int64_t l = 0;
                while (l < 255 && p + l < n && d[c + l] == d[p + l]) l++;
                if (l >= 4) {
                    toks[t++] = l;
                    toks[t++] = p - c;
                    matches++;
                    covered += l;
                    p += l;
                    continue;
                }
            }
        }
        toks[t++] = 0;
        toks[t++] = d[p];
        lits++;
        p++;
    }
    uint8_t *out = malloc(n);
    int64_t o = 0;
    for (int64_t k = 0; k < t; k += 2) {
        if (toks[k] == 0) {
            out[o++] = (uint8_t)toks[k + 1];
        } else {
            for (int64_t j = 0; j < toks[k]; j++, o++) out[o] = out[o - toks[k + 1]];
        }
    }
    printf("%lld %lld %lld %d\n", (long long)lits, (long long)matches, (long long)covered, o == n && memcmp(out, d, n) == 0);
    free(d);
    free(table);
    free(toks);
    free(out);
    return 0;
}
