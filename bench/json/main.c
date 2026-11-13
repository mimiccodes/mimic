/* JSON: encode 300,000 records to text, decode the text back into typed
 * records, check them. Hand-written encoder and parser for this record
 * shape (C has no JSON library): snprintf/strtod for numbers. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { char *p; size_t n, cap; } buf;
typedef struct { long id; char *name; double score; char **tags; long ntags; } item;

static void put(buf *b, const char *s, size_t n) {
    if (b->n + n + 1 > b->cap) {
        b->cap = (b->n + n + 1) * 2;
        b->p = realloc(b->p, b->cap);
    }
    memcpy(b->p + b->n, s, n);
    b->n += n;
}
static void puts_(buf *b, const char *s) { put(b, s, strlen(s)); }

static void put_str(buf *b, const char *s) {
    put(b, "\"", 1);
    for (; *s; s++) {
        if (*s == '"' || *s == '\\') put(b, "\\", 1);
        put(b, s, 1);
    }
    put(b, "\"", 1);
}

static void encode(buf *b, const item *xs, long n) {
    char tmp[64];
    put(b, "[", 1);
    for (long i = 0; i < n; i++) {
        if (i) put(b, ",", 1);
        puts_(b, "{\"id\":");
        snprintf(tmp, sizeof tmp, "%ld", xs[i].id);
        puts_(b, tmp);
        puts_(b, ",\"name\":");
        put_str(b, xs[i].name);
        puts_(b, ",\"score\":");
        snprintf(tmp, sizeof tmp, "%.17g", xs[i].score);
        puts_(b, tmp);
        puts_(b, ",\"tags\":[");
        for (long k = 0; k < xs[i].ntags; k++) {
            if (k) put(b, ",", 1);
            put_str(b, xs[i].tags[k]);
        }
        puts_(b, "]}");
    }
    put(b, "]", 1);
}

typedef struct { const char *s; size_t i; } parser;

static void ws(parser *p) { while (p->s[p->i] == ' ' || p->s[p->i] == '\n' || p->s[p->i] == '\t' || p->s[p->i] == '\r') p->i++; }
static void expect(parser *p, char c) {
    ws(p);
    if (p->s[p->i] != c) { fprintf(stderr, "expected %c at %zu\n", c, p->i); exit(1); }
    p->i++;
}
static char *str(parser *p) {
    expect(p, '"');
    buf b = {0};
    while (p->s[p->i] != '"') {
        if (p->s[p->i] == '\\') p->i++;
        put(&b, p->s + p->i, 1);
        p->i++;
    }
    p->i++;
    put(&b, "", 1);
    return b.p;
}
static double num(parser *p) {
    ws(p);
    char *end;
    double x = strtod(p->s + p->i, &end);
    p->i = (size_t)(end - p->s);
    return x;
}

static item *decode(const char *text, long *count) {
    parser p = {text, 0};
    long n = 0, cap = 16;
    item *xs = malloc(sizeof(item) * cap);
    expect(&p, '[');
    ws(&p);
    while (p.s[p.i] != ']') {
        if (n == cap) xs = realloc(xs, sizeof(item) * (cap *= 2));
        item it = {0};
        expect(&p, '{');
        for (;;) {
            char *key = str(&p);
            expect(&p, ':');
            if (!strcmp(key, "id")) it.id = (long)num(&p);
            else if (!strcmp(key, "name")) it.name = str(&p);
            else if (!strcmp(key, "score")) it.score = num(&p);
            else if (!strcmp(key, "tags")) {
                long tc = 4;
                it.tags = malloc(sizeof(char *) * tc);
                expect(&p, '[');
                ws(&p);
                while (p.s[p.i] != ']') {
                    if (it.ntags == tc) it.tags = realloc(it.tags, sizeof(char *) * (tc *= 2));
                    it.tags[it.ntags++] = str(&p);
                    ws(&p);
                    if (p.s[p.i] == ',') p.i++;
                    ws(&p);
                }
                p.i++;
            }
            free(key);
            ws(&p);
            if (p.s[p.i] == ',') { p.i++; continue; }
            expect(&p, '}');
            break;
        }
        xs[n++] = it;
        ws(&p);
        if (p.s[p.i] == ',') p.i++;
        ws(&p);
    }
    *count = n;
    return xs;
}

static void free_items(item *xs, long n) {
    for (long i = 0; i < n; i++) {
        free(xs[i].name);
        for (long k = 0; k < xs[i].ntags; k++) free(xs[i].tags[k]);
        free(xs[i].tags);
    }
    free(xs);
}

int main(void) {
    long n = 300000;
    item *xs = malloc(sizeof(item) * n);
    char tmp[32];
    for (long i = 0; i < n; i++) {
        snprintf(tmp, sizeof tmp, "item-%ld", i);
        xs[i].id = i;
        xs[i].name = strdup(tmp);
        xs[i].score = i * 0.5;
        xs[i].ntags = 2;
        xs[i].tags = malloc(sizeof(char *) * 2);
        xs[i].tags[0] = strdup("red");
        snprintf(tmp, sizeof tmp, "tag-%ld", i % 100);
        xs[i].tags[1] = strdup(tmp);
    }
    buf b = {0};
    encode(&b, xs, n);
    put(&b, "", 1);
    long m;
    item *back = decode(b.p, &m);
    long ids = 0, chars = 0, good = 0;
    for (long i = 0; i < m; i++) {
        ids += back[i].id;
        chars += (long)strlen(back[i].name);
        for (long k = 0; k < back[i].ntags; k++) chars += (long)strlen(back[i].tags[k]);
        if (back[i].score == back[i].id * 0.5) good++;
    }
    printf("%ld %ld %ld %ld\n", m, ids, chars, good);
    free_items(xs, n);
    free_items(back, m);
    free(b.p);
    return 0;
}
