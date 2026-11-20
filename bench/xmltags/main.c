// xmltags: an XML document of 300,000 elements in 5 tags, nested up to 12
// deep (a random walk from splitmix64), written and read back with one
// scan. Prints how many elements have each tag, the deepest nesting, and
// the sum of the ids.
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

static const char *TAGS[5] = {"item", "group", "note", "ref", "entry"};

int main(void) {
    char *text = malloc(64 * 1000000), *p = text;
    p += sprintf(p, "<doc>");
    int stack[16], depth = 0;
    for (int64_t k = 0, made = 0; made < 300000; k++) {
        if ((mix(k + 10000000) % 3 != 0 && depth < 11) || depth == 0) {
            int tag = (int)(mix(k) % 5);
            p += sprintf(p, "<%s id=\"%lld\">t", TAGS[tag], (long long)k);
            stack[depth++] = tag;
            made++;
        } else p += sprintf(p, "</%s>", TAGS[stack[--depth]]);
    }
    while (depth) p += sprintf(p, "</%s>", TAGS[stack[--depth]]);
    p += sprintf(p, "</doc>");
    int64_t counts[5] = {0}, deepest = 0, ids = 0, d = 0;
    for (char *q = text; (q = strchr(q, '<'));) {
        q++;
        if (*q == '/') {
            d--;
            continue;
        }
        d++;
        if (d > deepest) deepest = d;
        if (strncmp(q, "doc>", 4) == 0) continue;
        for (int t = 0; t < 5; t++) {
            size_t l = strlen(TAGS[t]);
            if (strncmp(q, TAGS[t], l) == 0 && (q[l] == ' ' || q[l] == '>')) counts[t]++;
        }
        char *id = strstr(q, "id=\"");
        ids += strtoll(id + 4, NULL, 10);
    }
    printf("%lld %lld %lld %lld %lld %lld %lld\n", (long long)counts[0], (long long)counts[1], (long long)counts[2], (long long)counts[3],
           (long long)counts[4], (long long)deepest, (long long)ids);
    return 0;
}
