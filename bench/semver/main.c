// semver: 1,000,000 version strings (MAJOR.MINOR.PATCH with an optional
// pre-release, from splitmix64) written into one text, then parsed line by
// line into one number each that orders as Semantic Versioning 2.0.0
// does, sorted, and written back. Prints the first, the median and the
// last version, and how many have a pre-release.
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

static const char *KINDS[3] = {"alpha", "beta", "rc"};

static int64_t num(const char **p) {
    int64_t v = 0;
    while (**p >= '0' && **p <= '9') v = v * 10 + *(*p)++ - '0';
    return v;
}

// the order as one number (see main.mim)
static int64_t key(const char *p) {
    int64_t major = num(&p);
    p++;
    int64_t minor = num(&p);
    p++;
    int64_t patch = num(&p);
    int64_t base = (major * 100 + minor) * 1000 + patch;
    if (*p != '-') return (base * 2 + 1) * 400;
    p++;
    int64_t kind = 0;
    for (int k = 0; k < 3; k++) {
        size_t l = strlen(KINDS[k]);
        if (strncmp(p, KINDS[k], l) == 0 && (p[l] == '.' || p[l] == '\n' || p[l] == 0)) {
            kind = k;
            p += l;
            break;
        }
    }
    int64_t n = 0;
    if (*p == '.') {
        p++;
        n = num(&p) + 1;
    }
    return base * 2 * 400 + kind * 100 + n;
}

static void text_of(int64_t k, char *out) {
    int64_t base = k / 800;
    int l = sprintf(out, "%lld.%lld.%lld", (long long)(base / 100000), (long long)(base / 1000 % 100), (long long)(base % 1000));
    if (k % 800 >= 400) return;
    int64_t kind = k % 400 / 100, n = k % 100;
    l += sprintf(out + l, "-%s", KINDS[kind]);
    if (n) sprintf(out + l, ".%lld", (long long)(n - 1));
}

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

int main(void) {
    static const char *pre_fmt[10] = {"", "", "", "", "", "", "-alpha.%lld", "-beta.%lld", "-rc.%lld", "-alpha"};
    char *text = malloc(40 * (size_t)N), *p = text;
    for (int64_t i = 0; i < N; i++) {
        if (i) *p++ = '\n';
        p += sprintf(p, "%lld.%lld.%lld", (long long)(mix(i) % 20), (long long)(mix(i + 10000000) % 30), (long long)(mix(i + 20000000) % 200));
        p += sprintf(p, pre_fmt[mix(i + 30000000) % 10], (long long)(mix(i + 40000000) % 20));
    }
    *p = 0;
    int64_t *keys = malloc(sizeof(int64_t) * N), m = 0;
    for (char *line = text; line && *line; ) {
        keys[m++] = key(line);
        line = strchr(line, '\n');
        if (line) line++;
    }
    qsort(keys, m, sizeof(int64_t), cmp);
    int64_t pre = 0;
    for (int64_t i = 0; i < m; i++) pre += keys[i] % 800 < 400;
    char a[64], b[64], c[64];
    text_of(keys[0], a);
    text_of(keys[m / 2], b);
    text_of(keys[m - 1], c);
    printf("%s %s %s %lld\n", a, b, c, (long long)pre);
    return 0;
}
