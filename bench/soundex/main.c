// soundex: 1,000,000 generated names coded by American Soundex, each code
// one number (letter × 1000 + digits); the codes sorted and counted in
// runs. Prints how many distinct codes there are, the most common one and
// how often it occurs, and the sum of the digits of all the codes.
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 1000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int digit(char c) {
    if (strchr("bfpv", c)) return 1;
    if (strchr("cgjkqsxz", c)) return 2;
    if (strchr("dt", c)) return 3;
    if (c == 'l') return 4;
    if (strchr("mn", c)) return 5;
    if (c == 'r') return 6;
    return 0;
}

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

int main(void) {
    int64_t *codes = malloc(sizeof(int64_t) * N);
    char name[16];
    for (int64_t k = 0; k < N; k++) {
        int64_t len = 4 + mix(k) % 7;
        for (int64_t j = 0; j < len; j++) name[j] = (char)(97 + mix(16 * k + j) % 26);
        name[0] = (char)toupper(name[0]);
        name[len] = 0;
        // the code, from the name in lower case
        char first = (char)tolower(name[0]);
        int last = digit(first), got = 0;
        int64_t out = 0;
        for (int64_t i = 1; i < len && got < 3; i++) {
            char c = (char)tolower(name[i]);
            if (c == 'h' || c == 'w') continue;
            int d = digit(c);
            if (d && d != last) {
                out = out * 10 + d;
                got++;
            }
            last = d;
        }
        for (; got < 3; got++) out *= 10;
        codes[k] = (first - 'a') * 1000 + out;
    }
    qsort(codes, N, sizeof(int64_t), cmp);
    int64_t distinct = 0, most = 0, top = 0, digits = 0;
    for (int64_t i = 0; i < N;) {
        int64_t j = i;
        while (j < N && codes[j] == codes[i]) j++;
        distinct++;
        if (j - i > most) {
            most = j - i;
            top = codes[i];
        }
        i = j;
    }
    for (int64_t i = 0; i < N; i++) digits += codes[i] % 10 + codes[i] / 10 % 10 + codes[i] / 100 % 10;
    printf("%lld %c%03lld %lld %lld\n", (long long)distinct, (char)('A' + top / 1000), (long long)(top % 1000), (long long)most, (long long)digits);
    return 0;
}
