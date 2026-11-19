// ini: an INI file of 100,000 sections (host, port, timeout, enabled and
// name in varying order, comments and blank lines between) written into
// one text and read back line by line. Prints the number of sections, the
// number of keys, the sum of the ports, how many sections are enabled, the
// total length of the names, and the sum of each port times its section's
// place in the file.
#include <ctype.h>
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

// [a, b) without surrounding spaces
static void trim(const char **a, const char **b) {
    while (*a < *b && isspace((unsigned char)**a)) (*a)++;
    while (*b > *a && isspace((unsigned char)(*b)[-1])) (*b)--;
}

static int is(const char *a, const char *b, const char *word) {
    size_t l = strlen(word);
    return (size_t)(b - a) == l && memcmp(a, word, l) == 0;
}

int main(void) {
    char *text = malloc(200 * 100000), *p = text;
    for (int64_t s = 0; s < 100000; s++) {
        char vals[5][64];
        sprintf(vals[0], "host = h%lld.example.com", (long long)(mix(s) % 1000));
        sprintf(vals[1], "port = %lld", (long long)(1024 + mix(s + 1) % 60000));
        sprintf(vals[2], "timeout = %lld", (long long)(mix(s + 2) % 120));
        sprintf(vals[3], "enabled = %s", mix(s + 3) % 3 ? "true" : "false");
        int nl = (int)(1 + mix(s + 4) % 20);
        sprintf(vals[4], "name = %.*s", nl, "xxxxxxxxxxxxxxxxxxxx");
        int r = (int)(mix(s + 5) % 5);
        if (s) *p++ = '\n';
        if (s % 7 == 0) p += sprintf(p, "; section %lld\n", (long long)s);
        else if (s % 11 == 0) p += sprintf(p, "# next\n");
        p += sprintf(p, "[server.%lld]\n", (long long)s);
        for (int k = 0; k < 5; k++) p += sprintf(p, "%s\n", vals[(k + r) % 5]);
    }
    *p = 0;
    int64_t sections = 0, keys = 0, ports = 0, enabled = 0, names = 0, weighted = 0;
    for (const char *line = text; *line;) {
        const char *end = strchr(line, '\n');
        if (!end) end = line + strlen(line);
        const char *a = line, *b = end;
        trim(&a, &b);
        if (a < b && *a == '[') sections++;
        else if (a < b && *a != ';' && *a != '#') {
            const char *eq = memchr(a, '=', b - a);
            if (eq) {
                const char *ka = a, *kb = eq, *va = eq + 1, *vb = b;
                trim(&ka, &kb);
                trim(&va, &vb);
                keys++;
                if (is(ka, kb, "port")) {
                    int64_t v = strtoll(va, NULL, 10);
                    ports += v;
                    weighted += v * sections;
                } else if (is(ka, kb, "enabled")) enabled += is(va, vb, "true");
                else if (is(ka, kb, "name")) names += vb - va;
            }
        }
        line = *end ? end + 1 : end;
    }
    printf("%lld %lld %lld %lld %lld %lld\n", (long long)sections, (long long)keys, (long long)ports, (long long)enabled, (long long)names,
           (long long)weighted);
    return 0;
}
