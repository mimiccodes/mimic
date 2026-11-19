// wordwrap: 2,000,000 words (from splitmix64) joined by spaces into one
// text, then wrapped greedily at 72 columns. Prints the number of lines,
// the sum over the lines of the square of the room left, and the length of
// the longest line.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 2000000
#define WIDTH 72

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    char *text = malloc(14 * (size_t)N), *p = text;
    for (int64_t k = 0; k < N; k++) {
        if (k) *p++ = ' ';
        char c = (char)(97 + mix(k) % 26);
        for (int64_t j = 0, l = 1 + mix(k) % 12; j < l; j++) *p++ = c;
    }
    *p = 0;
    int64_t line = 0, lines = 0, ragged = 0, longest = 0;
    for (char *q = text; *q;) {
        while (*q == ' ') q++;
        if (!*q) break;
        int64_t l = 0;
        while (q[l] && q[l] != ' ') l++;
        q += l;
        if (line == 0) line = l;
        else if (line + 1 + l <= WIDTH) line += 1 + l;
        else {
            lines++;
            ragged += (WIDTH - line) * (WIDTH - line);
            if (line > longest) longest = line;
            line = l;
        }
    }
    lines++;
    ragged += (WIDTH - line) * (WIDTH - line);
    if (line > longest) longest = line;
    printf("%lld %lld %lld\n", (long long)lines, (long long)ragged, (long long)longest);
    return 0;
}
