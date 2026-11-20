// csv: 400,000 rows of generated CSV (id, name, score, city; a city with a
// comma is quoted, RFC 4180), parsed back: the sum of the scores, the total
// length of the city names, and how many fields were quoted.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const int64_t n = 400000;
    char *text = malloc(n * 48), *w = text;
    for (int64_t i = 0; i < n; i++) {
        const char *city = i % 7 == 0 ? "\"Springfield, IL\"" : i % 3 == 0 ? "Paris" : "Rome";
        w += sprintf(w, "%s%lld,user%lld,%lld,%s", i ? "\n" : "", (long long)i, (long long)(i % 1000), (long long)(i * 7919 % 1000), city);
    }
    int64_t score = 0, city = 0, quoted = 0;
    const char *p = text, *end = w;
    while (p < end) {
        // one line: fields split at commas outside quotes
        const char *f[4];
        int64_t len[4];
        int k = 0;
        while (p < end && *p != '\n') {
            if (*p == '"') {
                const char *q = memchr(p + 1, '"', end - p - 1);
                if (k < 4) { f[k] = p + 1; len[k] = q - p - 1; }
                p = q + 1;
            } else {
                const char *s = p;
                while (p < end && *p != ',' && *p != '\n') p++;
                if (k < 4) { f[k] = s; len[k] = p - s; }
            }
            k++;
            if (p < end && *p == ',') p++;
        }
        if (p < end) p++;
        score += strtoll(f[2], NULL, 10);
        city += len[3];
        quoted += memchr(f[3], ',', len[3]) != NULL;
    }
    printf("%lld %lld %lld\n", (long long)score, (long long)city, (long long)quoted);
    free(text);
    return 0;
}
