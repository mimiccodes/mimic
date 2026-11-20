// url: 300,000 generated URLs split into their parts as RFC 3986 reads
// them; prints the sum of the ports, the path segments, the query
// parameters, the total host length and the sum of the `id` parameters.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const int64_t n = 300000;
    char u[256];
    int64_t ports = 0, segs = 0, params = 0, hosts = 0, ids = 0;
    for (int64_t i = 0; i < n; i++) {
        int m = snprintf(u, sizeof u, "https://user%lld:pw@host%lld.example.com:%lld/api/v%lld/items/%lld?id=%lld&page=%lld&sort=asc#sec%lld",
                         (long long)(i % 10), (long long)(i % 50), (long long)(8000 + i % 100), (long long)(i % 3), (long long)i, (long long)i, (long long)(i % 20), (long long)(i % 5));
        const char *end = u + m;
        const char *rest = strstr(u, "://") + 3;
        const char *slash = memchr(rest, '/', end - rest);
        const char *auth_end = slash ? slash : end;
        const char *at = memchr(rest, '@', auth_end - rest);
        const char *host = at ? at + 1 : rest;
        const char *colon = memchr(host, ':', auth_end - host);
        const char *host_end = colon ? colon : auth_end;
        hosts += host_end - host;
        if (colon) ports += strtoll(colon + 1, NULL, 10);
        const char *hash = memchr(auth_end, '#', end - auth_end);
        const char *q_end = hash ? hash : end;
        const char *qm = memchr(auth_end, '?', q_end - auth_end);
        const char *path_end = qm ? qm : q_end;
        for (const char *p = auth_end; p < path_end;) {
            const char *s = p + 1, *e = memchr(s, '/', path_end - s);
            if (!e) e = path_end;
            segs += e > s;
            p = e;
        }
        if (qm) {
            for (const char *p = qm + 1; p < q_end;) {
                const char *e = memchr(p, '&', q_end - p);
                if (!e) e = q_end;
                params++;
                if (e - p > 3 && memcmp(p, "id=", 3) == 0) ids += strtoll(p + 3, NULL, 10);
                p = e + 1;
            }
        }
    }
    printf("%lld %lld %lld %lld %lld\n", (long long)ports, (long long)segs, (long long)params, (long long)hosts, (long long)ids);
    return 0;
}
