// wordfreq: about 1,000,000 generated words with spaces, commas, full
// stops and line breaks. The words are found where letters start and
// stop, each hashed with FNV-1a, the hashes sorted and the runs of equal
// ones counted. Prints the words, the distinct words, and how often the
// most common one occurs.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int cmp(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

int main(void) {
    static const char cons[] = "bcdfghklmnprstvz", vowels[] = "aeiou";
    char *t = malloc(1000000 * 10);
    int64_t n = 0;
    for (int64_t i = 0; i < 1000000; i++) {
        int64_t v = mix(2 * i) % (1 + mix(2 * i + 1) % 6000);
        for (int64_t k = 0; k < 1 + v % 4; k++) {
            t[n++] = cons[(v >> (3 * k + 2)) % 16];
            t[n++] = vowels[(v >> (3 * k + 7)) % 5];
        }
        if (i % 15 == 14) t[n++] = '\n';
        else if (i % 13 == 12) { t[n++] = '.'; t[n++] = ' '; }
        else if (i % 13 == 5) { t[n++] = ','; t[n++] = ' '; }
        else t[n++] = ' ';
    }
    uint64_t *h = malloc(sizeof(uint64_t) * 1000000);
    int64_t m = 0;
    for (int64_t i = 0; i < n;) {
        if (t[i] < 'a' || t[i] > 'z') { i++; continue; }
        uint64_t x = 0xCBF29CE484222325ull;
        while (i < n && t[i] >= 'a' && t[i] <= 'z') x = (x ^ (uint8_t)t[i++]) * 0x100000001B3ull;
        h[m++] = x;
    }
    qsort(h, m, sizeof(uint64_t), cmp);
    int64_t distinct = 0, most = 0, run = 0;
    for (int64_t i = 0; i < m; i++) {
        if (i == 0 || h[i] != h[i - 1]) { distinct++; run = 0; }
        if (++run > most) most = run;
    }
    printf("%lld %lld %lld\n", (long long)m, (long long)distinct, (long long)most);
    free(t); free(h);
    return 0;
}
