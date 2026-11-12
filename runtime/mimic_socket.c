/* Networking: TCP sockets, an HTTP/1.1 client and server.
 * Included at the end of mimic_rt.c (same translation unit), after mimic_io.c.
 *
 * These intrinsics return the standard library's own values (`Ok(…)`,
 * `Err("…")`, `Response { … }`) built from the type descriptors the compiler
 * exports by name (`mim_desc_Ok`, …), so no wrapper code is needed and a
 * handler passed to `serve` is visible to the compiler's effect analysis. */

/* ok_of, err_of, obj_owned: see mimic_io.c */

extern const mim_desc mim_desc_Socket, mim_desc_Listener, mim_desc_Request, mim_desc_Response;

static mim_val *err_errno(const char *what, const char *detail) {
    char buf[512];
    if (detail) snprintf(buf, sizeof buf, "%s %s: %s", what, detail, strerror(errno));
    else snprintf(buf, sizeof buf, "%s: %s", what, strerror(errno));
    return err_of(buf);
}

/* field i of a record (borrowed) */
static mim_val *field_of(mim_val *o, const mim_desc *d, int64_t i, const char *what) {
    if (!o || o->dtype != MIM_OBJ || o->rank != 0 || o->desc != d || i >= o->len) {
        char buf[128];
        snprintf(buf, sizeof buf, "%s: expected a %s", what, d->type_name);
        mim_panic(buf);
    }
    return ((mim_val **)o->data)[i];
}

#if defined(_WIN32) || defined(__wasi__)
#define MIM_NO_SOCKETS 1
#endif

#ifdef MIM_NO_SOCKETS

static mim_val *no_net(void) { return err_of("networking is not available on this platform"); }
mim_val *mim_std_tcp_connect(mim_val *h, mim_val *p) { (void)h; (void)p; return no_net(); }
mim_val *mim_std_tcp_listen(mim_val *p) { (void)p; return no_net(); }
mim_val *mim_std_tcp_accept(mim_val *l) { (void)l; return no_net(); }
mim_val *mim_std_tcp_send(mim_val *s, mim_val *t) { (void)s; (void)t; return no_net(); }
mim_val *mim_std_tcp_receive(mim_val *s, mim_val *n) { (void)s; (void)n; return no_net(); }
mim_val *mim_std_tcp_close(mim_val *s) { (void)s; return NULL; }
mim_val *mim_std_tcp_send_bytes(mim_val *s, mim_val *b) { (void)s; (void)b; return no_net(); }
mim_val *mim_std_tcp_receive_bytes(mim_val *s, mim_val *n) { (void)s; (void)n; return no_net(); }
mim_val *mim_std_http_request(mim_val *m, mim_val *u, mim_val *h, mim_val *b) { (void)m; (void)u; (void)h; (void)b; return no_net(); }
mim_val *mim_std_http_fetch(mim_val *u) { (void)u; return no_net(); }
mim_val *mim_std_http_serve(mim_val *p, mim_val *h) { (void)p; (void)h; return no_net(); }
mim_val *mim_std_http_serve_requests(mim_val *p, mim_val *h, mim_val *n) { (void)p; (void)h; (void)n; return no_net(); }

#else

#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <netdb.h>

#ifdef MSG_NOSIGNAL
#define MIM_SEND_FLAGS MSG_NOSIGNAL
#else
#define MIM_SEND_FLAGS 0
#endif

static void sock_setup(int fd) {
#ifdef SO_NOSIGPIPE
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, sizeof one); /* a closed peer is an error, not a signal */
#endif
    int on = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &on, sizeof on);
}

static void sock_timeout(int fd, int seconds) {
    struct timeval tv = {seconds, 0};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
}

/* a connected socket, or -1 with an error message */
static int dial(const char *host, int64_t port, char *err, size_t errn) {
    char ps[16];
    snprintf(ps, sizeof ps, "%lld", (long long)port);
    struct addrinfo hints = {0}, *res = NULL;
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int rc = getaddrinfo(host, ps, &hints, &res);
    if (rc != 0) {
        snprintf(err, errn, "cannot resolve %s: %s", host, gai_strerror(rc));
        return -1;
    }
    int fd = -1, saved = 0;
    for (struct addrinfo *a = res; a; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd < 0) {
            saved = errno;
            continue;
        }
        if (connect(fd, a->ai_addr, a->ai_addrlen) == 0) break;
        saved = errno;
        close(fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0) {
        snprintf(err, errn, "cannot connect to %s:%lld: %s", host, (long long)port, strerror(saved));
        return -1;
    }
    sock_setup(fd);
    return fd;
}

static int send_all(int fd, const char *p, size_t n) {
    while (n > 0) {
        ssize_t w = send(fd, p, n, MIM_SEND_FLAGS);
        if (w < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        p += w;
        n -= (size_t)w;
    }
    return 0;
}

static mim_val *socket_of(int64_t fd) {
    mim_val *f = mim_int(fd);
    return obj_owned(&mim_desc_Socket, 1, &f);
}

static int fd_of(mim_val *s, const char *what) {
    if (s && s->dtype == MIM_OBJ && (s->desc == &mim_desc_Socket || s->desc == &mim_desc_Listener))
        return (int)geti(((mim_val **)s->data)[0], 0);
    char buf[96];
    snprintf(buf, sizeof buf, "%s: expected a Socket", what);
    mim_panic(buf);
    return -1;
}

/* ---- TCP ---------------------------------------------------------------- */

mim_val *mim_std_tcp_connect(mim_val *host, mim_val *port) {
    char err[512];
    int fd = dial(io_path(host, "tcp_connect"), geti(port, 0), err, sizeof err);
    if (fd < 0) return err_of(err);
    return ok_of(socket_of(fd));
}

/* listen on every interface; port 0 picks a free port (see `.port`) */
mim_val *mim_std_tcp_listen(mim_val *port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return err_errno("cannot listen", NULL);
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    a.sin_port = htons((uint16_t)geti(port, 0));
    char what[64];
    snprintf(what, sizeof what, "port %lld", (long long)geti(port, 0));
    if (bind(fd, (struct sockaddr *)&a, sizeof a) != 0 || listen(fd, 512) != 0) {
        mim_val *e = err_errno("cannot listen on", what);
        close(fd);
        return e;
    }
    socklen_t len = sizeof a;
    getsockname(fd, (struct sockaddr *)&a, &len);
    mim_val *fields[2] = {mim_int(fd), mim_int(ntohs(a.sin_port))};
    return ok_of(obj_owned(&mim_desc_Listener, 2, fields));
}

mim_val *mim_std_tcp_accept(mim_val *l) {
    int lfd = fd_of(l, "tcp_accept");
    int fd;
    while ((fd = accept(lfd, NULL, NULL)) < 0 && errno == EINTR) {
    }
    if (fd < 0) return err_errno("cannot accept", NULL);
    sock_setup(fd);
    return ok_of(socket_of(fd));
}

mim_val *mim_std_tcp_send(mim_val *s, mim_val *text) {
    int fd = fd_of(s, "tcp::send");
    if (!text || text->dtype != MIM_CHAR || text->rank != 1) mim_panic("tcp::send: expected text (str)");
    if (send_all(fd, (const char *)text->data, (size_t)text->len) != 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int(text->len));
}

/* up to `max` bytes; "" when the other side has closed the connection */
mim_val *mim_std_tcp_receive(mim_val *s, mim_val *max) {
    int fd = fd_of(s, "tcp::receive");
    int64_t n = geti(max, 0);
    if (n <= 0) n = 65536;
    char *buf = (char *)malloc((size_t)n);
    ssize_t r;
    while ((r = recv(fd, buf, (size_t)n, 0)) < 0 && errno == EINTR) {
    }
    if (r < 0) {
        free(buf);
        return err_errno("cannot receive", NULL);
    }
    mim_val *v = mim_str(buf, (int64_t)r);
    free(buf);
    return ok_of(v);
}

mim_val *mim_std_tcp_send_bytes(mim_val *s, mim_val *b) {
    int fd = fd_of(s, "tcp::send_bytes");
    if (!b || b->rank != 1 || (b->dtype != MIM_U8 && b->len != 0)) mim_panic("tcp::send_bytes: expected bytes ([u8])");
    if (send_all(fd, (const char *)b->data, (size_t)b->len) != 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int(b->len));
}

mim_val *mim_std_tcp_receive_bytes(mim_val *s, mim_val *limit) {
    mim_val *r = mim_std_tcp_receive(s, limit);
    if (r->desc != &mim_desc_Ok) return r;
    mim_val *t = ((mim_val **)r->data)[0];
    mim_val *b = alloc(MIM_U8, 1, t->len);
    if (t->len) memcpy(b->data, t->data, (size_t)t->len);
    mim_release(r);
    return ok_of(b);
}

mim_val *mim_std_tcp_close(mim_val *s) {
    close(fd_of(s, "tcp::close"));
    return NULL;
}

/* ---- HTTP: shared parsing ----------------------------------------------- */

/* read until the end of the headers; returns the offset just past "\r\n\r\n"
 * (or -1), leaving any body bytes already read in `b` */
static int64_t read_head(int fd, sbuf *b) {
    char tmp[16384];
    for (;;) {
        for (size_t i = b->n >= 4 ? 0 : 0; i + 3 < b->n; i++)
            if (b->p[i] == '\r' && b->p[i + 1] == '\n' && b->p[i + 2] == '\r' && b->p[i + 3] == '\n') return (int64_t)i + 4;
        if (b->n > (1 << 20)) return -1; /* headers over 1 MB */
        ssize_t r = recv(fd, tmp, sizeof tmp, 0);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) return -1;
        sb_put(b, tmp, (size_t)r);
    }
}

static int ieq(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        char x = a[i], y = b[i];
        if (x >= 'A' && x <= 'Z') x = (char)(x - 'A' + 'a');
        if (y >= 'A' && y <= 'Z') y = (char)(y - 'A' + 'a');
        if (x != y) return 0;
    }
    return 1;
}

/* headers between `p` and `end` (lines after the first) as a map with
 * lower-case names; also reports Content-Length and chunked encoding */
static mim_val *parse_headers(const char *p, const char *end, int64_t *content_length, int *chunked) {
    *content_length = -1;
    *chunked = 0;
    int64_t cap = 16, n = 0;
    mim_val **keys = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
    mim_val **vals = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
    while (p < end) {
        const char *eol = p;
        while (eol + 1 < end && !(eol[0] == '\r' && eol[1] == '\n')) eol++;
        if (eol + 1 >= end) eol = end;
        const char *colon = memchr(p, ':', (size_t)(eol - p));
        if (colon) {
            const char *v = colon + 1;
            while (v < eol && (*v == ' ' || *v == '\t')) v++;
            const char *ve = eol;
            while (ve > v && (ve[-1] == ' ' || ve[-1] == '\t')) ve--;
            size_t kn = (size_t)(colon - p);
            char *k = (char *)malloc(kn + 1);
            for (size_t i = 0; i < kn; i++) k[i] = (char)(p[i] >= 'A' && p[i] <= 'Z' ? p[i] - 'A' + 'a' : p[i]);
            k[kn] = 0;
            if (kn == 14 && memcmp(k, "content-length", 14) == 0) *content_length = atoll(v);
            if (kn == 17 && memcmp(k, "transfer-encoding", 17) == 0 && (size_t)(ve - v) >= 7 && ieq(ve - 7, "chunked", 7)) *chunked = 1;
            if (n == cap) {
                cap *= 2;
                keys = (mim_val **)realloc(keys, sizeof(mim_val *) * (size_t)cap);
                vals = (mim_val **)realloc(vals, sizeof(mim_val *) * (size_t)cap);
            }
            keys[n] = mim_str(k, (int64_t)kn);
            vals[n] = mim_str(v, (int64_t)(ve - v));
            n++;
            free(k);
        }
        p = eol + 2;
    }
    mim_val *m = mim_map_lit(n, keys, vals);
    for (int64_t i = 0; i < n; i++) {
        mim_release(keys[i]);
        mim_release(vals[i]);
    }
    free(keys);
    free(vals);
    return m;
}

/* read the rest of a body: `have` bytes are in b->p[off..]; returns the body */
static mim_val *read_body(int fd, sbuf *b, int64_t off, int64_t content_length, int chunked, int to_eof) {
    char tmp[65536];
    if (content_length >= 0) {
        while ((int64_t)b->n - off < content_length) {
            ssize_t r = recv(fd, tmp, sizeof tmp, 0);
            if (r < 0 && errno == EINTR) continue;
            if (r <= 0) break;
            sb_put(b, tmp, (size_t)r);
        }
        int64_t have = (int64_t)b->n - off;
        return mim_str(b->p + off, have < content_length ? have : content_length);
    }
    if (chunked) {
        /* read to the end first (we sent Connection: close), then decode */
        for (;;) {
            ssize_t r = recv(fd, tmp, sizeof tmp, 0);
            if (r < 0 && errno == EINTR) continue;
            if (r <= 0) break;
            sb_put(b, tmp, (size_t)r);
        }
        sbuf out = {0};
        const char *p = b->p + off, *end = b->p + b->n;
        while (p < end) {
            long long size = strtoll(p, NULL, 16);
            const char *nl = memchr(p, '\n', (size_t)(end - p));
            if (!nl || size <= 0) break;
            p = nl + 1;
            if (end - p < size) size = end - p;
            sb_put(&out, p, (size_t)size);
            p += size + 2;
        }
        mim_val *v = mim_str(out.p ? out.p : "", (int64_t)out.n);
        free(out.p);
        return v;
    }
    if (to_eof) {
        for (;;) {
            ssize_t r = recv(fd, tmp, sizeof tmp, 0);
            if (r < 0 && errno == EINTR) continue;
            if (r <= 0) break;
            sb_put(b, tmp, (size_t)r);
        }
    }
    return mim_str(b->p + off, (int64_t)b->n - off);
}

/* ---- HTTP client -------------------------------------------------------- */

mim_val *mim_std_http_request(mim_val *method, mim_val *url, mim_val *headers, mim_val *body) {
    const char *m = io_path(method, "request"), *u = io_path(url, "request");
    if (strncmp(u, "https://", 8) == 0) return err_of("https needs TLS, which this version of mimic does not support yet");
    if (strncmp(u, "http://", 7) != 0) return err_of("request: the URL must start with http://");
    const char *hp = u + 7, *slash = strchr(hp, '/');
    size_t hl = slash ? (size_t)(slash - hp) : strlen(hp);
    char host[256];
    if (hl == 0 || hl >= sizeof host) return err_of("request: bad host in URL");
    memcpy(host, hp, hl);
    host[hl] = 0;
    int64_t port = 80;
    char *colon = strrchr(host, ':');
    if (colon && !strchr(colon, ']')) {
        port = atoll(colon + 1);
        *colon = 0;
    }
    const char *path = slash ? slash : "/";
    char err[512];
    int fd = dial(host, port, err, sizeof err);
    if (fd < 0) return err_of(err);
    sock_timeout(fd, 60);
    sbuf q = {0};
    char line[1024];
    /* Host: the host (and port) exactly as written in the URL */
    snprintf(line, sizeof line, "%s %s HTTP/1.1\r\nHost: %.*s\r\nConnection: close\r\nUser-Agent: mimic\r\n", m, path, (int)hl, hp);
    sb_put(&q, line, strlen(line));
    int has_len = 0;
    if (headers && headers->dtype == MIM_MAP) {
        mim_val **h = (mim_val **)headers->data;
        int64_t count = ((const int64_t *)(const void *)headers->desc)[0];
        for (int64_t i = 0; i < count; i++) {
            mim_val *k = h[2 * i], *v = h[2 * i + 1];
            if (k->dtype != MIM_CHAR || v->dtype != MIM_CHAR) continue;
            if (k->len == 14 && ieq((const char *)k->data, "content-length", 14)) has_len = 1;
            sb_put(&q, (const char *)k->data, (size_t)k->len);
            sb_put(&q, ": ", 2);
            sb_put(&q, (const char *)v->data, (size_t)v->len);
            sb_put(&q, "\r\n", 2);
        }
    }
    int64_t blen = body && body->dtype == MIM_CHAR ? body->len : 0;
    if (!has_len && (blen > 0 || strcmp(m, "POST") == 0 || strcmp(m, "PUT") == 0)) {
        snprintf(line, sizeof line, "Content-Length: %lld\r\n", (long long)blen);
        sb_put(&q, line, strlen(line));
    }
    sb_put(&q, "\r\n", 2);
    if (blen) sb_put(&q, (const char *)body->data, (size_t)blen);
    int bad = send_all(fd, q.p, q.n) != 0;
    free(q.p);
    if (bad) {
        mim_val *e = err_errno("cannot send the request to", host);
        close(fd);
        return e;
    }
    sbuf b = {0};
    int64_t off = read_head(fd, &b);
    if (off < 0) {
        free(b.p);
        close(fd);
        return err_of("no HTTP response (connection closed or timed out)");
    }
    /* status line: HTTP/1.1 200 OK */
    const char *sp = memchr(b.p, ' ', (size_t)off);
    int64_t status = sp ? atoll(sp + 1) : 0;
    const char *first_end = strstr(b.p, "\r\n");
    int64_t clen;
    int chunked;
    mim_val *hm = parse_headers(first_end + 2, b.p + off - 2, &clen, &chunked);
    int no_body = strcmp(m, "HEAD") == 0 || status == 204 || status == 304;
    mim_val *bodyv = no_body ? mim_str("", 0) : read_body(fd, &b, off, clen, chunked, 1);
    close(fd);
    free(b.p);
    mim_val *fields[3] = {mim_int(status), hm, bodyv};
    return ok_of(obj_owned(&mim_desc_Response, 3, fields));
}

mim_val *mim_std_http_fetch(mim_val *url) {
    mim_val *m = mim_str("GET", 3), *h = mim_map_lit(0, NULL, NULL), *b = mim_str("", 0);
    mim_val *r = mim_std_http_request(m, url, h, b);
    mim_release(m);
    mim_release(h);
    mim_release(b);
    return r;
}

/* ---- HTTP server -------------------------------------------------------- */

static const char *reason(int64_t s) {
    switch (s) {
    case 200: return "OK";
    case 201: return "Created";
    case 204: return "No Content";
    case 301: return "Moved Permanently";
    case 302: return "Found";
    case 304: return "Not Modified";
    case 400: return "Bad Request";
    case 401: return "Unauthorized";
    case 403: return "Forbidden";
    case 404: return "Not Found";
    case 405: return "Method Not Allowed";
    case 500: return "Internal Server Error";
    case 503: return "Service Unavailable";
    default: return "Status";
    }
}

/* one request on a connection: parse, call the handler, answer, close */
static void serve_one(int fd, mim_val *handler) {
    sock_timeout(fd, 30);
    sbuf b = {0};
    int64_t off = read_head(fd, &b);
    if (off < 0) {
        free(b.p);
        close(fd);
        return;
    }
    const char *line_end = strstr(b.p, "\r\n");
    const char *sp1 = memchr(b.p, ' ', (size_t)(line_end - b.p));
    const char *sp2 = sp1 ? memchr(sp1 + 1, ' ', (size_t)(line_end - sp1 - 1)) : NULL;
    if (!sp1 || !sp2) {
        const char *bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        send_all(fd, bad, strlen(bad));
        free(b.p);
        close(fd);
        return;
    }
    const char *target = sp1 + 1, *q = memchr(target, '?', (size_t)(sp2 - target));
    int64_t clen;
    int chunked;
    mim_val *hm = parse_headers(line_end + 2, b.p + off - 2, &clen, &chunked);
    mim_val *bodyv = read_body(fd, &b, off, clen < 0 && !chunked ? 0 : clen, chunked, 0);
    mim_val *fields[5] = {
        mim_str(b.p, (int64_t)(sp1 - b.p)),
        mim_str(target, (int64_t)((q ? q : sp2) - target)),
        q ? mim_str(q + 1, (int64_t)(sp2 - q - 1)) : mim_str("", 0),
        hm,
        bodyv,
    };
    mim_val *req = obj_owned(&mim_desc_Request, 5, fields);
    free(b.p);
    mim_val *resp = mim_call(handler, 1, &req);
    mim_release(req);
    int64_t status = geti(field_of(resp, &mim_desc_Response, 0, "an HTTP handler's result"), 0);
    mim_val *rh = field_of(resp, &mim_desc_Response, 1, "an HTTP handler's result");
    mim_val *rb = field_of(resp, &mim_desc_Response, 2, "an HTTP handler's result");
    sbuf out = {0};
    char line[256];
    snprintf(line, sizeof line, "HTTP/1.1 %lld %s\r\n", (long long)status, reason(status));
    sb_put(&out, line, strlen(line));
    int has_type = 0;
    if (rh && rh->dtype == MIM_MAP) {
        mim_val **h = (mim_val **)rh->data;
        int64_t count = ((const int64_t *)(const void *)rh->desc)[0];
        for (int64_t i = 0; i < count; i++) {
            mim_val *k = h[2 * i], *v = h[2 * i + 1];
            if (k->dtype != MIM_CHAR || v->dtype != MIM_CHAR) continue;
            if (k->len == 12 && ieq((const char *)k->data, "content-type", 12)) has_type = 1;
            if (k->len == 14 && ieq((const char *)k->data, "content-length", 14)) continue;
            sb_put(&out, (const char *)k->data, (size_t)k->len);
            sb_put(&out, ": ", 2);
            sb_put(&out, (const char *)v->data, (size_t)v->len);
            sb_put(&out, "\r\n", 2);
        }
    }
    if (!has_type) sb_str(&out, "Content-Type: text/plain; charset=utf-8\r\n");
    int64_t blen = rb && rb->dtype == MIM_CHAR ? rb->len : 0;
    snprintf(line, sizeof line, "Content-Length: %lld\r\nConnection: close\r\n\r\n", (long long)blen);
    sb_put(&out, line, strlen(line));
    if (blen) sb_put(&out, (const char *)rb->data, (size_t)blen);
    send_all(fd, out.p, out.n);
    free(out.p);
    mim_release(resp);
    shutdown(fd, SHUT_WR);
    close(fd);
}

typedef struct {
    int fd;
    mim_val *handler;
} conn_job;

static void *conn_main(void *p) {
    conn_job *j = (conn_job *)p;
    serve_one(j->fd, j->handler);
    mim_release(j->handler);
    free(j);
    free_list_drain(); /* this thread ends now */
    return NULL;
}

/* accept `limit` connections (0 = forever); handlers whose effects may
 * overlap get a thread each, others run one at a time in order */
static mim_val *serve_loop(mim_val *port, mim_val *handler, int64_t limit) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("serve: the handler must be a function");
    mim_val *lr = mim_std_tcp_listen(port);
    if (lr->desc != &mim_desc_Ok) return lr;
    mim_val *l = ((mim_val **)lr->data)[0];
    int lfd = (int)geti(((mim_val **)l->data)[0], 0);
    mim_release(lr);
    int parallel = MIM_THREADS && FN_DESC(handler)->pure;
    int64_t served = 0;
    pthread_t *threads = limit > 0 && parallel ? (pthread_t *)calloc((size_t)limit, sizeof(pthread_t)) : NULL;
    while (limit == 0 || served < limit) {
        int fd = accept(lfd, NULL, NULL);
        if (fd < 0) {
            if (errno == EINTR || errno == ECONNABORTED) continue;
            break;
        }
        sock_setup(fd);
        if (parallel) {
            conn_job *j = (conn_job *)malloc(sizeof(conn_job));
            j->fd = fd;
            j->handler = handler;
            mim_retain(handler);
            pthread_t t;
            pthread_attr_t a;
            pthread_attr_init(&a);
            pthread_attr_setstacksize(&a, (size_t)32 << 20);
            if (pthread_create(&t, &a, conn_main, j) != 0) {
                conn_main(j);
            } else if (threads) {
                threads[served] = t;
            } else {
                pthread_detach(t);
            }
            pthread_attr_destroy(&a);
        } else {
            serve_one(fd, handler);
        }
        served++;
    }
    if (threads) {
        for (int64_t i = 0; i < served; i++)
            if (threads[i]) pthread_join(threads[i], NULL);
        free(threads);
    }
    close(lfd);
    return ok_of(mim_int(served));
}

mim_val *mim_std_http_serve(mim_val *port, mim_val *handler) { return serve_loop(port, handler, 0); }

mim_val *mim_std_http_serve_requests(mim_val *port, mim_val *handler, mim_val *count) {
    return serve_loop(port, handler, geti(count, 0) > 0 ? geti(count, 0) : 1);
}

#endif
