// http_parse: 200,000 HTTP/1.1 requests back to back in one stream, read
// as a server reads them (request line, headers to the blank line, then
// Content-Length bytes of body). Prints the requests, the headers, the
// body bytes, the total path length and the POSTs.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>

int main(void) {
    const int64_t n = 200000;
    char *buf = malloc(n * 256), *w = buf;
    const char *methods[3] = {"GET", "POST", "PUT"};
    for (int64_t i = 0; i < n; i++) {
        const char *m = methods[i % 3];
        w += sprintf(w, "%s /api/items/%lld HTTP/1.1\r\nHost: example.com\r\nUser-Agent: bench/%lld\r\nAccept: */*\r\n", m, (long long)i, (long long)(i % 7));
        if (i % 3 == 0) {
            w += sprintf(w, "\r\n");
        } else {
            char body[64], xs[20];
            memset(xs, 'x', i % 17);
            xs[i % 17] = 0;
            int bl = snprintf(body, sizeof body, "{\"id\":%lld,\"v\":\"%s\"}", (long long)i, xs);
            w += sprintf(w, "Content-Type: application/json\r\nContent-Length: %d\r\n\r\n%s", bl, body);
        }
    }
    const char *p = buf, *end = w;
    int64_t reqs = 0, headers = 0, bodies = 0, paths = 0, posts = 0;
    while (p < end) {
        // the request line
        const char *eol = strstr(p, "\r\n");
        const char *sp1 = memchr(p, ' ', eol - p), *sp2 = memchr(sp1 + 1, ' ', eol - sp1 - 1);
        reqs++;
        paths += sp2 - sp1 - 1;
        posts += sp1 - p == 4 && memcmp(p, "POST", 4) == 0;
        p = eol + 2;
        // headers, to the blank line
        int64_t clen = 0;
        while (!(p[0] == '\r' && p[1] == '\n')) {
            eol = strstr(p, "\r\n");
            headers++;
            if (strncasecmp(p, "Content-Length: ", 16) == 0) clen = strtoll(p + 16, NULL, 10);
            p = eol + 2;
        }
        p += 2 + clen;
        bodies += clen;
    }
    printf("%lld %lld %lld %lld %lld\n", (long long)reqs, (long long)headers, (long long)bodies, (long long)paths, (long long)posts);
    free(buf);
    return 0;
}
