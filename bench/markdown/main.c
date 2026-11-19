// markdown: 400,000 lines of a Markdown subset (from splitmix64) written as
// one text and read back line by line, words classified by their first
// character. Prints the headings at each level, the list items, the bold
// spans, the links, the code spans and the plain words.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static const char *WORDS[8] = {"array", "kernel", "fusion", "memory", "vector", "thread", "cache", "loop"};

static char *word(char *p, int64_t k, int64_t j) {
    const char *w = WORDS[mix(16 * k + j) % 8];
    int64_t r = mix(16 * k + j + 8) % 10;
    if (r == 0) return p + sprintf(p, "**%s**", w);
    if (r == 1) return p + sprintf(p, "`%s`", w);
    if (r == 2) return p + sprintf(p, "[%s](https://x.example/%lld)", w, (long long)k);
    return p + sprintf(p, "%s", w);
}

int main(void) {
    char *text = malloc(200 * 400000), *p = text;
    for (int64_t k = 0; k < 400000; k++) {
        if (k) *p++ = '\n';
        int64_t r = mix(k + 100000000) % 10;
        if (r == 0) {
            for (int64_t h = 0; h < 1 + mix(k) % 3; h++) *p++ = '#';
            *p++ = ' ';
            for (int j = 0; j < 3; j++) p += sprintf(p, j ? " %s" : "%s", WORDS[mix(k + j) % 8]);
        } else if (r != 1) {
            if (r < 4) p += sprintf(p, "- ");
            for (int64_t j = 0, n = 2 + mix(k) % 8; j < n; j++) {
                if (j) *p++ = ' ';
                p = word(p, k, j);
            }
        }
    }
    *p = 0;
    int64_t heads[4] = {0}, items = 0, bold = 0, links = 0, code = 0, plain = 0;
    for (char *l = text; l;) {
        char *end = strchr(l, '\n');
        if (end) *end = 0;
        char *body = l;
        int level = 0;
        while (body[level] == '#') level++;
        if (level && body[level] == ' ') {
            heads[level]++;
            body += level + 1;
        } else if (body[0] == '-' && body[1] == ' ') {
            items++;
            body += 2;
        }
        for (char *w = strtok(body, " "); w; w = strtok(NULL, " ")) {
            if (*w == '*') bold++;
            else if (*w == '[') links++;
            else if (*w == '`') code++;
            else plain++;
        }
        l = end ? end + 1 : NULL;
    }
    printf("%lld %lld %lld %lld %lld %lld %lld %lld\n", (long long)heads[1], (long long)heads[2], (long long)heads[3], (long long)items, (long long)bold,
           (long long)links, (long long)code, (long long)plain);
    return 0;
}
