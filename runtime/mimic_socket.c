/* Networking: TCP sockets, an HTTP/1.1 client and server.
 * Included at the end of mimic_rt.c (same translation unit), after mimic_io.c.
 *
 * These intrinsics return the standard library's own values (`Ok(…)`,
 * `Err("…")`, `Response { … }`) built from the type descriptors the compiler
 * exports by name (`mim_desc_Ok`, …), so no wrapper code is needed and a
 * handler passed to `serve` is visible to the compiler's effect analysis. */

/* ok_of, err_of, obj_owned: see mimic_io.c */

extern const mim_desc mim_desc_tcp_Socket, mim_desc_tcp_Listener, mim_desc_http_Request, mim_desc_http_Response;

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
mim_val *mim_std_ws_connect(mim_val *u) { (void)u; return no_net(); }
mim_val *mim_std_ws_send(mim_val *s, mim_val *t) { (void)s; (void)t; return no_net(); }
mim_val *mim_std_ws_send_bytes(mim_val *s, mim_val *b) { (void)s; (void)b; return no_net(); }
mim_val *mim_std_ws_receive(mim_val *s) { (void)s; return no_net(); }
mim_val *mim_std_ws_close(mim_val *s) { (void)s; return NULL; }
mim_val *mim_std_ws_serve(mim_val *p, mim_val *h) { (void)p; (void)h; return no_net(); }
mim_val *mim_std_ws_serve_connections(mim_val *p, mim_val *h, mim_val *n) { (void)p; (void)h; (void)n; return no_net(); }
mim_val *mim_std_tcp_receive_bytes(mim_val *s, mim_val *n) { (void)s; (void)n; return no_net(); }
mim_val *mim_std_udp_bind(mim_val *p) { (void)p; return no_net(); }
mim_val *mim_std_udp_send(mim_val *s, mim_val *h, mim_val *p, mim_val *t) { (void)s; (void)h; (void)p; (void)t; return no_net(); }
mim_val *mim_std_udp_send_bytes(mim_val *s, mim_val *h, mim_val *p, mim_val *b) { (void)s; (void)h; (void)p; (void)b; return no_net(); }
mim_val *mim_std_udp_receive(mim_val *s) { (void)s; return no_net(); }
mim_val *mim_std_udp_receive_within(mim_val *s, mim_val *d) { (void)s; (void)d; return no_net(); }
mim_val *mim_std_udp_close(mim_val *s) { (void)s; return NULL; }
mim_val *mim_std_tls_identity(mim_val *c, mim_val *k) { (void)c; (void)k; return no_net(); }
mim_val *mim_std_tls_connect(mim_val *h, mim_val *p) { (void)h; (void)p; return no_net(); }
mim_val *mim_std_tls_connect_trusting(mim_val *h, mim_val *p, mim_val *c) { (void)h; (void)p; (void)c; return no_net(); }
mim_val *mim_std_tls_accept(mim_val *l, mim_val *i) { (void)l; (void)i; return no_net(); }
mim_val *mim_std_tls_send(mim_val *s, mim_val *t) { (void)s; (void)t; return no_net(); }
mim_val *mim_std_tls_send_bytes(mim_val *s, mim_val *b) { (void)s; (void)b; return no_net(); }
mim_val *mim_std_tls_receive(mim_val *s, mim_val *n) { (void)s; (void)n; return no_net(); }
mim_val *mim_std_tls_receive_bytes(mim_val *s, mim_val *n) { (void)s; (void)n; return no_net(); }
mim_val *mim_std_tls_close(mim_val *s) { (void)s; return NULL; }
mim_val *mim_std_tls_self_signed(mim_val *h) { (void)h; return no_net(); }
mim_val *mim_std_ui_serve_app(mim_val *p, mim_val *a, mim_val *b, mim_val *n) { (void)p; (void)a; (void)b; (void)n; return no_net(); }
mim_val *mim_std_ui_client_script(void) { return mim_str(MIM_UI_JS, (int64_t)strlen(MIM_UI_JS)); }
mim_val *mim_std_http_serve_tls(mim_val *p, mim_val *h, mim_val *i) { (void)p; (void)h; (void)i; return no_net(); }
mim_val *mim_std_http_serve_tls_requests(mim_val *p, mim_val *h, mim_val *i, mim_val *n) { (void)p; (void)h; (void)i; (void)n; return no_net(); }
mim_val *mim_std_ws_serve_tls(mim_val *p, mim_val *h, mim_val *i) { (void)p; (void)h; (void)i; return no_net(); }
mim_val *mim_std_ws_serve_tls_connections(mim_val *p, mim_val *h, mim_val *i, mim_val *n) { (void)p; (void)h; (void)i; (void)n; return no_net(); }
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

/* connect; on the event loop the coroutine waits for the connection
 * instead of blocking its thread */
static int connect_co(int fd, const struct sockaddr *addr, socklen_t len) {
#if MIM_GREEN
    if (t_co) {
        int fl = fcntl(fd, F_GETFL);
        fcntl(fd, F_SETFL, fl | O_NONBLOCK);
        int rc = connect(fd, addr, len);
        if (rc != 0 && errno == EINPROGRESS) {
            co_wait_fd(fd, 1, 0);
            int e = 0;
            socklen_t el = sizeof e;
            getsockopt(fd, SOL_SOCKET, SO_ERROR, &e, &el);
            rc = e ? -1 : 0;
            errno = e;
        }
        int saved = errno;
        fcntl(fd, F_SETFL, fl);
        errno = saved;
        return rc;
    }
#endif
    return connect(fd, addr, len);
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
        if (connect_co(fd, a->ai_addr, a->ai_addrlen) == 0) break;
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
        ssize_t w = msend(fd, p, n, MIM_SEND_FLAGS);
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
    return obj_owned(&mim_desc_tcp_Socket, 1, &f);
}

static int fd_of(mim_val *s, const char *what) {
    if (s && s->dtype == MIM_OBJ && (s->desc == &mim_desc_tcp_Socket || s->desc == &mim_desc_tcp_Listener))
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
    return ok_of(obj_owned(&mim_desc_tcp_Listener, 2, fields));
}

mim_val *mim_std_tcp_accept(mim_val *l) {
    int lfd = fd_of(l, "tcp_accept");
    int fd;
    mim_wait_readable(lfd);
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
    while ((r = mrecv(fd, buf, (size_t)n)) < 0 && errno == EINTR) {
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

/* ---- connections: plain or TLS ------------------------------------------ */

typedef struct {
    int fd;
    void *tls; /* a TLS session (tls_open), or NULL */
    /* a server whose handler has effects: held while the handler runs, so
     * handlers take turns (reading the request does not) */
    pthread_mutex_t *turn;
} mconn;

static ssize_t tls_read(void *t, void *buf, size_t n);
static int tls_write_all(void *t, const char *p, size_t n);

static ssize_t conn_recv(mconn *c, void *buf, size_t n) {
    if (c->tls) return tls_read(c->tls, buf, n);
    return mrecv(c->fd, buf, n);
}

static int conn_send_all(mconn *c, const char *p, size_t n) {
    if (c->tls) return tls_write_all(c->tls, p, n);
    return send_all(c->fd, p, n);
}

static void tls_close(void *t);

static void conn_close(mconn *c) {
    if (c->tls) tls_close(c->tls);
    c->tls = NULL;
    close(c->fd);
}

/* ---- DER and PEM: just enough to write keys and certificates ---------- */

/* random bytes from the operating system */
__attribute__((unused)) static void arc4random_buf_or_urandom(void *p, size_t n) {
#if defined(__APPLE__)
    arc4random_buf(p, n);
#else
    size_t got = 0;
    while (got < n) {
        size_t k = n - got > 256 ? 256 : n - got;
        if (getentropy((char *)p + got, k) != 0) break;
        got += k;
    }
#endif
}

typedef struct {
    uint8_t *p;
    size_t n, cap;
} dbuf;

__attribute__((unused)) static void db_put(dbuf *b, const void *p, size_t n) {
    if (b->n + n > b->cap) {
        b->cap = (b->n + n) * 2 + 64;
        b->p = (uint8_t *)realloc(b->p, b->cap);
    }
    memcpy(b->p + b->n, p, n);
    b->n += n;
}

__attribute__((unused)) static void db_tlv(dbuf *b, uint8_t tag, const uint8_t *p, size_t n) {
    uint8_t h[6];
    size_t k = 0;
    h[k++] = tag;
    if (n < 128) h[k++] = (uint8_t)n;
    else if (n < 256) { h[k++] = 0x81; h[k++] = (uint8_t)n; }
    else if (n < 65536) { h[k++] = 0x82; h[k++] = (uint8_t)(n >> 8); h[k++] = (uint8_t)n; }
    else { h[k++] = 0x83; h[k++] = (uint8_t)(n >> 16); h[k++] = (uint8_t)(n >> 8); h[k++] = (uint8_t)n; }
    db_put(b, h, k);
    db_put(b, p, n);
}

__attribute__((unused)) static char *pem_wrap(const char *label, const uint8_t *der, size_t n, size_t *outn) {
    static const char tab[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    size_t cap = n * 2 + 128;
    char *o = (char *)malloc(cap);
    size_t k = (size_t)snprintf(o, cap, "-----BEGIN %s-----\n", label);
    int col = 0;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)der[i] << 16 | (i + 1 < n ? (uint32_t)der[i + 1] << 8 : 0) | (i + 2 < n ? der[i + 2] : 0);
        o[k++] = tab[v >> 18 & 63];
        o[k++] = tab[v >> 12 & 63];
        o[k++] = i + 1 < n ? tab[v >> 6 & 63] : '=';
        o[k++] = i + 2 < n ? tab[v & 63] : '=';
        if ((col += 4) == 64) {
            o[k++] = '\n';
            col = 0;
        }
    }
    if (col) o[k++] = '\n';
    k += (size_t)snprintf(o + k, cap - k, "-----END %s-----\n", label);
    *outn = k;
    return o;
}


/* TLS with the operating system's own library, loaded on first use (no
 * link-time dependency): Apple's Security framework, or OpenSSL 3 on Linux.
 * Certificates and host names are checked against the system's trust store. */

#if defined(__APPLE__)

typedef int32_t OSStatus;
typedef OSStatus (*st_io_fn)(const void *conn, void *data, size_t *len);
static struct {
    int ready;
    void *(*create)(const void *alloc, int side, int type);
    OSStatus (*set_io)(void *ctx, st_io_fn rd, st_io_fn wr);
    OSStatus (*set_conn)(void *ctx, const void *conn);
    OSStatus (*set_peer)(void *ctx, const char *name, size_t len);
    OSStatus (*handshake)(void *ctx);
    OSStatus (*write)(void *ctx, const void *data, size_t len, size_t *done);
    OSStatus (*read)(void *ctx, void *data, size_t len, size_t *done);
    OSStatus (*close)(void *ctx);
    void (*release)(const void *cf);
} st;
static pthread_mutex_t st_lock = PTHREAD_MUTEX_INITIALIZER;

#define ERR_WOULD_BLOCK (-9803)
#define ERR_CLOSED (-9805)
#define ERR_CLOSED_ABORT (-9806)
#define ERR_CLOSED_NO_NOTIFY (-9816)

static OSStatus st_read_cb(const void *conn, void *data, size_t *len) {
    int fd = (int)(intptr_t)conn;
    size_t want = *len, got = 0;
    while (got < want) {
        ssize_t r = mrecv(fd, (char *)data + got, want - got);
        if (r > 0) got += (size_t)r;
        else if (r == 0) { *len = got; return ERR_CLOSED; }
        else if (errno == EINTR) continue;
        else { *len = got; return errno == EAGAIN || errno == EWOULDBLOCK ? ERR_WOULD_BLOCK : -36; }
    }
    *len = got;
    return 0;
}

static OSStatus st_write_cb(const void *conn, void *data, size_t *len) {
    int fd = (int)(intptr_t)conn;
    size_t want = *len, put = 0;
    while (put < want) {
        ssize_t w = msend(fd, (const char *)data + put, want - put, MIM_SEND_FLAGS);
        if (w > 0) put += (size_t)w;
        else if (w < 0 && errno == EINTR) continue;
        else { *len = put; return errno == EAGAIN || errno == EWOULDBLOCK ? ERR_WOULD_BLOCK : -36; }
    }
    *len = put;
    return 0;
}

static int tls_load(void) {
    pthread_mutex_lock(&st_lock);
    if (!st.ready) {
        void *sec = dlopen("/System/Library/Frameworks/Security.framework/Security", RTLD_LAZY | RTLD_LOCAL);
        void *cf = dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", RTLD_LAZY | RTLD_LOCAL);
        if (sec && cf) {
            *(void **)&st.create = dlsym(sec, "SSLCreateContext");
            *(void **)&st.set_io = dlsym(sec, "SSLSetIOFuncs");
            *(void **)&st.set_conn = dlsym(sec, "SSLSetConnection");
            *(void **)&st.set_peer = dlsym(sec, "SSLSetPeerDomainName");
            *(void **)&st.handshake = dlsym(sec, "SSLHandshake");
            *(void **)&st.write = dlsym(sec, "SSLWrite");
            *(void **)&st.read = dlsym(sec, "SSLRead");
            *(void **)&st.close = dlsym(sec, "SSLClose");
            *(void **)&st.release = dlsym(cf, "CFRelease");
        }
        st.ready = st.create && st.set_io && st.set_conn && st.set_peer && st.handshake && st.write && st.read && st.close && st.release ? 1 : -1;
    }
    pthread_mutex_unlock(&st_lock);
    return st.ready == 1;
}

static const char *st_error(OSStatus e) {
    switch (e) {
    case -9807: return "the server's certificate chain is not valid";
    case -9808: return "the server's certificate is bad";
    case -9812: return "the server's certificate is not trusted (unknown root)";
    case -9813: return "the server's certificate has no trusted root";
    case -9814: return "the server's certificate has expired";
    case -9815: return "the server's certificate is not valid yet";
    case -9843: return "the server's certificate is for a different host name";
    case -9802: return "the server's certificate failed verification";
    case ERR_WOULD_BLOCK: return "timed out";
    default: return NULL;
    }
}

/* certificates, keys and identities: more of the Security framework, for
 * servers and for clients that trust one particular certificate */
static struct {
    int ready;
    const void *(*data_create)(const void *alloc, const uint8_t *bytes, long len);
    const void *(*array_create)(const void *alloc, const void **values, long n, const void *callbacks);
    const void *array_callbacks;
    long (*array_count)(const void *a);
    const void *(*array_at)(const void *a, long i);
    unsigned long (*type_id)(const void *cf);
    unsigned long (*cert_type)(void);
    unsigned long (*key_type)(void);
    OSStatus (*import)(const void *data, const void *name, uint32_t *format, uint32_t *type, uint32_t flags, const void *params, const void *keychain, const void **out);
    const void *(*identity_create)(const void *alloc, const void *cert, const void *key);
    OSStatus (*set_cert)(void *ctx, const void *certs);
    OSStatus (*set_option)(void *ctx, int option, uint8_t value);
    OSStatus (*copy_trust)(void *ctx, const void **trust);
    OSStatus (*set_anchors)(const void *trust, const void *anchors);
    OSStatus (*anchors_only)(const void *trust, uint8_t only);
    uint8_t (*evaluate)(const void *trust, const void **error);
} sx;

static int sx_load(void) {
    if (!tls_load()) return 0;
    pthread_mutex_lock(&st_lock);
    if (!sx.ready) {
        void *sec = dlopen("/System/Library/Frameworks/Security.framework/Security", RTLD_LAZY | RTLD_LOCAL);
        void *cf = dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", RTLD_LAZY | RTLD_LOCAL);
        if (sec && cf) {
            *(void **)&sx.data_create = dlsym(cf, "CFDataCreate");
            *(void **)&sx.array_create = dlsym(cf, "CFArrayCreate");
            sx.array_callbacks = dlsym(cf, "kCFTypeArrayCallBacks");
            *(void **)&sx.array_count = dlsym(cf, "CFArrayGetCount");
            *(void **)&sx.array_at = dlsym(cf, "CFArrayGetValueAtIndex");
            *(void **)&sx.type_id = dlsym(cf, "CFGetTypeID");
            *(void **)&sx.cert_type = dlsym(sec, "SecCertificateGetTypeID");
            *(void **)&sx.key_type = dlsym(sec, "SecKeyGetTypeID");
            *(void **)&sx.import = dlsym(sec, "SecItemImport");
            *(void **)&sx.identity_create = dlsym(sec, "SecIdentityCreate");
            *(void **)&sx.set_cert = dlsym(sec, "SSLSetCertificate");
            *(void **)&sx.set_option = dlsym(sec, "SSLSetSessionOption");
            *(void **)&sx.copy_trust = dlsym(sec, "SSLCopyPeerTrust");
            *(void **)&sx.set_anchors = dlsym(sec, "SecTrustSetAnchorCertificates");
            *(void **)&sx.anchors_only = dlsym(sec, "SecTrustSetAnchorCertificatesOnly");
            *(void **)&sx.evaluate = dlsym(sec, "SecTrustEvaluateWithError");
        }
        sx.ready = sx.data_create && sx.array_create && sx.array_callbacks && sx.array_count && sx.array_at && sx.type_id && sx.cert_type &&
                           sx.key_type && sx.import && sx.identity_create && sx.set_cert && sx.set_option && sx.copy_trust && sx.set_anchors &&
                           sx.anchors_only && sx.evaluate
                       ? 1
                       : -1;
    }
    pthread_mutex_unlock(&st_lock);
    return sx.ready == 1;
}

/* the certificates (want_key = 0) or the first private key (1) in PEM text,
 * as a CFArray; NULL when there are none */
static const void *sx_import(const char *pem, size_t n, int want_key) {
    const void *data = sx.data_create(NULL, (const uint8_t *)pem, (long)n);
    if (!data) return NULL;
    uint32_t format = 0, type = 0; /* unknown: let the framework tell */
    const void *items = NULL;
    OSStatus rc = sx.import(data, NULL, &format, &type, 0, NULL, NULL, &items);
    st.release(data);
    if (rc != 0 || !items) return NULL;
    long count = sx.array_count(items), k = 0;
    const void **keep = (const void **)malloc(sizeof(void *) * (size_t)(count ? count : 1));
    unsigned long want = want_key ? sx.key_type() : sx.cert_type();
    for (long i = 0; i < count; i++) {
        const void *it = sx.array_at(items, i);
        if (sx.type_id(it) == want) keep[k++] = it;
    }
    const void *out = k ? sx.array_create(NULL, keep, want_key ? 1 : k, sx.array_callbacks) : NULL;
    free(keep);
    st.release(items);
    return out;
}

/* The Security framework imports keys only in the older formats ("RSA
 * PRIVATE KEY", "EC PRIVATE KEY"); today's tools write PKCS#8 ("PRIVATE
 * KEY"). This unwraps PKCS#8 into the older form: an RSA key as it is, an
 * EC key with its curve put back inside (PKCS#8 keeps it outside). */


/* one DER element at p[*i]: its tag, and its contents [*start, *start+*len) */
static int der_next(const uint8_t *p, size_t n, size_t *i, uint8_t *tag, size_t *start, size_t *len) {
    if (*i + 2 > n) return 0;
    *tag = p[*i];
    size_t l = p[*i + 1], j = *i + 2;
    if (l & 0x80) {
        size_t k = l & 0x7f;
        if (k == 0 || k > 3 || j + k > n) return 0;
        l = 0;
        for (size_t m = 0; m < k; m++) l = l << 8 | p[j + m];
        j += k;
    }
    if (j + l > n) return 0;
    *start = j;
    *len = l;
    *i = j + l;
    return 1;
}

static int b64v(int c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
}

/* the DER inside the first PEM block labelled `label` (malloc'd), or NULL */
static uint8_t *pem_block(const char *pem, size_t n, const char *label, size_t *outn) {
    char begin[64], end[64];
    snprintf(begin, sizeof begin, "-----BEGIN %s-----", label);
    snprintf(end, sizeof end, "-----END %s-----", label);
    const char *b = NULL;
    for (size_t i = 0; i + strlen(begin) <= n; i++)
        if (memcmp(pem + i, begin, strlen(begin)) == 0) {
            b = pem + i + strlen(begin);
            break;
        }
    if (!b) return NULL;
    uint8_t *out = (uint8_t *)malloc(n);
    size_t k = 0;
    uint32_t acc = 0;
    int bits = 0;
    for (const char *q = b; q < pem + n; q++) {
        if (*q == '-') {
            if ((size_t)(pem + n - q) >= strlen(end) && memcmp(q, end, strlen(end)) == 0) {
                *outn = k;
                return out;
            }
            break;
        }
        int v = b64v((unsigned char)*q);
        if (v < 0) continue;
        acc = acc << 6 | (uint32_t)v;
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out[k++] = (uint8_t)(acc >> bits);
        }
    }
    free(out);
    return NULL;
}


/* PKCS#8 → "RSA PRIVATE KEY" / "EC PRIVATE KEY" PEM (malloc'd), or NULL */
static char *pkcs8_unwrap(const char *pem, size_t n, size_t *outn) {
    static const uint8_t rsa_oid[] = {0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01};
    static const uint8_t ec_oid[] = {0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01};
    size_t dn;
    uint8_t *d = pem_block(pem, n, "PRIVATE KEY", &dn);
    if (!d) return NULL;
    char *out = NULL;
    size_t i = 0, st, ln, a, an, ai, ks, kl;
    uint8_t tag;
    /* PrivateKeyInfo ::= SEQUENCE { version, AlgorithmIdentifier, OCTET STRING } */
    if (der_next(d, dn, &i, &tag, &st, &ln) && tag == 0x30) {
        size_t j = st, e = st + ln;
        int ok = der_next(d, e, &j, &tag, &a, &an) && tag == 0x02;     /* version */
        ok = ok && der_next(d, e, &j, &tag, &a, &an) && tag == 0x30;  /* algorithm */
        ok = ok && der_next(d, e, &j, &tag, &ks, &kl) && tag == 0x04; /* the key */
        size_t oid, oidn = 0, curve = 0, curven = 0;
        if (ok) {
            ai = a;
            ok = der_next(d, a + an, &ai, &tag, &oid, &oidn) && tag == 0x06;
            if (ok && ai < a + an && der_next(d, a + an, &ai, &tag, &curve, &curven) && tag != 0x06) curven = 0;
        }
        if (ok && oidn == sizeof rsa_oid && memcmp(d + oid, rsa_oid, oidn) == 0) {
            out = pem_wrap("RSA PRIVATE KEY", d + ks, kl, outn);
        } else if (ok && curven && oidn == sizeof ec_oid && memcmp(d + oid, ec_oid, oidn) == 0) {
            /* ECPrivateKey ::= SEQUENCE { version, OCTET STRING, [0] curve OPTIONAL, [1] public key OPTIONAL } */
            size_t q = ks, es, el;
            if (der_next(d, ks + kl, &q, &tag, &es, &el) && tag == 0x30) {
                dbuf body = {0}, curve_el = {0}, seq = {0};
                size_t r = es, cs, cl, start;
                int have_curve = 0;
                while (start = r, r < es + el && der_next(d, es + el, &r, &tag, &cs, &cl)) {
                    if (tag == 0xa0) have_curve = 1;
                    if (tag == 0xa1 && !have_curve) { /* the curve goes before the public key */
                        db_tlv(&curve_el, 0x06, d + curve, curven);
                        db_tlv(&body, 0xa0, curve_el.p, curve_el.n);
                        have_curve = 1;
                    }
                    db_put(&body, d + start, r - start);
                }
                if (!have_curve) {
                    db_tlv(&curve_el, 0x06, d + curve, curven);
                    db_tlv(&body, 0xa0, curve_el.p, curve_el.n);
                }
                db_tlv(&seq, 0x30, body.p, body.n);
                out = pem_wrap("EC PRIVATE KEY", seq.p, seq.n, outn);
                free(body.p);
                free(curve_el.p);
                free(seq.p);
            }
        }
    }
    free(d);
    return out;
}

/* a server identity (certificate chain + key) for SSLSetCertificate */
static void *tls_identity_load(const char *cert, size_t cn, const char *key, size_t kn, char *err, size_t errn) {
    if (!sx_load()) {
        snprintf(err, errn, "TLS servers are not available: the Security framework could not be loaded");
        return NULL;
    }
    const void *certs = sx_import(cert, cn, 0);
    if (!certs) {
        snprintf(err, errn, "tls::identity: no certificate found in the certificate text (expected PEM)");
        return NULL;
    }
    const void *keys = sx_import(key, kn, 1);
    if (!keys) { /* PKCS#8: unwrap it into a form the framework imports */
        size_t un;
        char *u = pkcs8_unwrap(key, kn, &un);
        if (u) {
            keys = sx_import(u, un, 1);
            free(u);
        }
    }
    if (!keys) {
        st.release(certs);
        snprintf(err, errn, "tls::identity: no private key found in the key text (expected PEM)");
        return NULL;
    }
    const void *id = sx.identity_create(NULL, sx.array_at(certs, 0), sx.array_at(keys, 0));
    st.release(keys);
    if (!id) {
        st.release(certs);
        snprintf(err, errn, "tls::identity: the key does not belong to the certificate");
        return NULL;
    }
    long n = sx.array_count(certs);
    const void **chain = (const void **)malloc(sizeof(void *) * (size_t)n);
    chain[0] = id;
    for (long i = 1; i < n; i++) chain[i] = sx.array_at(certs, i);
    const void *arr = sx.array_create(NULL, chain, n, sx.array_callbacks);
    free(chain);
    st.release(id);
    st.release(certs);
    return (void *)arr;
}

/* the certificates a client trusts instead of the system's */
static void *tls_trust_load(const char *pem, size_t n, char *err, size_t errn) {
    if (!sx_load()) {
        snprintf(err, errn, "TLS is not available: the Security framework could not be loaded");
        return NULL;
    }
    const void *certs = sx_import(pem, n, 0);
    if (!certs) snprintf(err, errn, "no certificate found in the trusted certificate text (expected PEM)");
    return (void *)certs;
}

static void tls_trust_free(void *t) {
    if (t) st.release(t);
}

static void tls_handshake_error(OSStatus rc, const char *host, char *err, size_t errn) {
    const char *why = st_error(rc);
    if (why) snprintf(err, errn, "TLS with %s failed: %s", host, why);
    else snprintf(err, errn, "TLS with %s failed (Security framework error %d)", host, (int)rc);
}

/* the server side of a TLS session on an accepted socket */
static void *tls_accept_on(int fd, void *identity, char *err, size_t errn) {
    void *ctx = st.create(NULL, 0 /* server */, 0 /* stream */);
    if (!ctx) {
        snprintf(err, errn, "TLS: cannot create a session");
        return NULL;
    }
    st.set_io(ctx, st_read_cb, st_write_cb);
    st.set_conn(ctx, (const void *)(intptr_t)fd);
    sx.set_cert(ctx, identity);
    OSStatus rc = st.handshake(ctx);
    if (rc != 0) {
        tls_handshake_error(rc, "the client", err, errn);
        st.release(ctx);
        return NULL;
    }
    return ctx;
}

/* a TLS session over a connected socket, or NULL with a message; `trust`
 * (from tls_trust_load) replaces the system's trusted certificates */
static void *tls_open_trusting(int fd, const char *host, void *trust, char *err, size_t errn) {
    if (!tls_load()) {
        snprintf(err, errn, "TLS is not available: the Security framework could not be loaded");
        return NULL;
    }
    void *ctx = st.create(NULL, 1 /* client */, 0 /* stream */);
    if (!ctx) {
        snprintf(err, errn, "TLS: cannot create a session");
        return NULL;
    }
    st.set_io(ctx, st_read_cb, st_write_cb);
    st.set_conn(ctx, (const void *)(intptr_t)fd);
    st.set_peer(ctx, host, strlen(host));
    if (trust) sx.set_option(ctx, 0 /* kSSLSessionOptionBreakOnServerAuth */, 1);
    OSStatus rc;
    while ((rc = st.handshake(ctx)) == -9841 /* errSSLPeerAuthCompleted */) {
        if (!trust) continue;
        const void *t = NULL;
        int ok = sx.copy_trust(ctx, &t) == 0 && t;
        if (ok) {
            sx.set_anchors(t, trust);
            sx.anchors_only(t, 1);
            ok = sx.evaluate(t, NULL);
        }
        if (t) st.release(t);
        if (!ok) {
            snprintf(err, errn, "TLS with %s failed: the server's certificate is not the trusted one (or not for this host name)", host);
            st.close(ctx);
            st.release(ctx);
            return NULL;
        }
    }
    if (rc != 0) {
        tls_handshake_error(rc, host, err, errn);
        st.release(ctx);
        return NULL;
    }
    return ctx;
}

static void *tls_open(int fd, const char *host, char *err, size_t errn) { return tls_open_trusting(fd, host, NULL, err, errn); }

static ssize_t tls_read(void *t, void *buf, size_t n) {
    size_t got = 0;
    OSStatus rc = st.read(t, buf, n, &got);
    if (got > 0) return (ssize_t)got;
    if (rc == ERR_CLOSED || rc == ERR_CLOSED_NO_NOTIFY || rc == ERR_CLOSED_ABORT) return 0;
    errno = EIO;
    return -1;
}

static int tls_write_all(void *t, const char *p, size_t n) {
    while (n > 0) {
        size_t done = 0;
        OSStatus rc = st.write(t, p, n, &done);
        if (rc != 0 && done == 0) return -1;
        p += done;
        n -= done;
    }
    return 0;
}

static void tls_close(void *t) {
    st.close(t);
    st.release(t);
}

#elif defined(__linux__)

static struct {
    int ready;
    void *ctx;
    void *(*method)(void);
    void *(*ctx_new)(void *);
    int (*default_paths)(void *);
    void (*set_verify)(void *, int, void *);
    void *(*ssl_new)(void *);
    int (*set_fd)(void *, int);
    long (*ctrl)(void *, int, long, void *);
    int (*set1_host)(void *, const char *);
    int (*connect)(void *);
    int (*read)(void *, void *, int);
    int (*write)(void *, const void *, int);
    int (*shutdown)(void *);
    void (*free)(void *);
    long (*verify_result)(const void *);
    const char *(*verify_string)(long);
    int (*pending)(const void *); /* optional: for the event loop */
    int (*get_fd)(const void *);
    /* servers and trusted certificates (optional) */
    void *(*server_method)(void);
    void *(*bio_mem)(const void *, int);
    int (*bio_free)(void *);
    void *(*read_x509)(void *, void *, void *, void *);
    void *(*read_key)(void *, void *, void *, void *);
    void (*x509_free)(void *);
    void (*pkey_free)(void *);
    int (*use_cert)(void *, void *);
    int (*use_key)(void *, void *);
    int (*check_key)(const void *);
    long (*ctx_ctrl)(void *, int, long, void *);
    void *(*cert_store)(const void *);
    int (*store_add)(void *, void *);
    void (*ctx_free)(void *);
    int (*accept)(void *);
    int (*get_error)(const void *, int);
} os;
static pthread_mutex_t os_lock = PTHREAD_MUTEX_INITIALIZER;

static int tls_load(void) {
    pthread_mutex_lock(&os_lock);
    if (!os.ready) {
        void *ssl = dlopen("libssl.so.3", RTLD_LAZY | RTLD_GLOBAL);
        void *crypto = dlopen("libcrypto.so.3", RTLD_LAZY | RTLD_GLOBAL);
        if (ssl) {
            *(void **)&os.method = dlsym(ssl, "TLS_client_method");
            *(void **)&os.ctx_new = dlsym(ssl, "SSL_CTX_new");
            *(void **)&os.default_paths = dlsym(ssl, "SSL_CTX_set_default_verify_paths");
            *(void **)&os.set_verify = dlsym(ssl, "SSL_CTX_set_verify");
            *(void **)&os.ssl_new = dlsym(ssl, "SSL_new");
            *(void **)&os.set_fd = dlsym(ssl, "SSL_set_fd");
            *(void **)&os.ctrl = dlsym(ssl, "SSL_ctrl");
            *(void **)&os.set1_host = dlsym(ssl, "SSL_set1_host");
            *(void **)&os.connect = dlsym(ssl, "SSL_connect");
            *(void **)&os.read = dlsym(ssl, "SSL_read");
            *(void **)&os.write = dlsym(ssl, "SSL_write");
            *(void **)&os.shutdown = dlsym(ssl, "SSL_shutdown");
            *(void **)&os.free = dlsym(ssl, "SSL_free");
            *(void **)&os.verify_result = dlsym(ssl, "SSL_get_verify_result");
            *(void **)&os.pending = dlsym(ssl, "SSL_pending");
            *(void **)&os.get_fd = dlsym(ssl, "SSL_get_fd");
            *(void **)&os.server_method = dlsym(ssl, "TLS_server_method");
            *(void **)&os.use_cert = dlsym(ssl, "SSL_CTX_use_certificate");
            *(void **)&os.use_key = dlsym(ssl, "SSL_CTX_use_PrivateKey");
            *(void **)&os.check_key = dlsym(ssl, "SSL_CTX_check_private_key");
            *(void **)&os.ctx_ctrl = dlsym(ssl, "SSL_CTX_ctrl");
            *(void **)&os.cert_store = dlsym(ssl, "SSL_CTX_get_cert_store");
            *(void **)&os.ctx_free = dlsym(ssl, "SSL_CTX_free");
            *(void **)&os.accept = dlsym(ssl, "SSL_accept");
            *(void **)&os.get_error = dlsym(ssl, "SSL_get_error");
            if (crypto) {
                *(void **)&os.bio_mem = dlsym(crypto, "BIO_new_mem_buf");
                *(void **)&os.bio_free = dlsym(crypto, "BIO_free");
                *(void **)&os.read_x509 = dlsym(crypto, "PEM_read_bio_X509");
                *(void **)&os.read_key = dlsym(crypto, "PEM_read_bio_PrivateKey");
                *(void **)&os.x509_free = dlsym(crypto, "X509_free");
                *(void **)&os.pkey_free = dlsym(crypto, "EVP_PKEY_free");
                *(void **)&os.store_add = dlsym(crypto, "X509_STORE_add_cert");
            }
            if (crypto) *(void **)&os.verify_string = dlsym(crypto, "X509_verify_cert_error_string");
        }
        int ok = os.method && os.ctx_new && os.default_paths && os.set_verify && os.ssl_new && os.set_fd && os.ctrl && os.set1_host &&
                 os.connect && os.read && os.write && os.shutdown && os.free && os.verify_result;
        if (ok) {
            os.ctx = os.ctx_new(os.method());
            ok = os.ctx != NULL;
            if (ok) {
                os.default_paths(os.ctx);
                os.set_verify(os.ctx, 1 /* SSL_VERIFY_PEER */, NULL);
            }
        }
        os.ready = ok ? 1 : -1;
    }
    pthread_mutex_unlock(&os_lock);
    return os.ready == 1;
}

static int os_extra(void) {
    return os.server_method && os.bio_mem && os.bio_free && os.read_x509 && os.read_key && os.x509_free && os.pkey_free && os.use_cert &&
           os.use_key && os.check_key && os.ctx_ctrl && os.cert_store && os.store_add && os.ctx_free && os.accept;
}

/* a server identity: an SSL_CTX with the certificate chain and key */
static void *tls_identity_load(const char *cert, size_t cn, const char *key, size_t kn, char *err, size_t errn) {
    if (!tls_load() || !os_extra()) {
        snprintf(err, errn, "TLS servers are not available: install OpenSSL 3 (libssl.so.3); static builds cannot load it");
        return NULL;
    }
    void *ctx = os.ctx_new(os.server_method());
    if (!ctx) {
        snprintf(err, errn, "TLS: cannot create a server context");
        return NULL;
    }
    void *bio = os.bio_mem(cert, (int)cn);
    void *x = bio ? os.read_x509(bio, NULL, NULL, NULL) : NULL;
    int ok = x && os.use_cert(ctx, x) == 1;
    if (x) os.x509_free(x);
    if (ok) /* the rest of the chain: the context owns them */
        while ((x = os.read_x509(bio, NULL, NULL, NULL)) != NULL) os.ctx_ctrl(ctx, 14 /* SSL_CTRL_EXTRA_CHAIN_CERT */, 0, x);
    if (bio) os.bio_free(bio);
    if (!ok) {
        os.ctx_free(ctx);
        snprintf(err, errn, "tls::identity: no certificate found in the certificate text (expected PEM)");
        return NULL;
    }
    bio = os.bio_mem(key, (int)kn);
    void *k = bio ? os.read_key(bio, NULL, NULL, NULL) : NULL;
    if (bio) os.bio_free(bio);
    ok = k && os.use_key(ctx, k) == 1;
    if (k) os.pkey_free(k);
    if (!ok) {
        os.ctx_free(ctx);
        snprintf(err, errn, "tls::identity: no private key found in the key text (expected PEM)");
        return NULL;
    }
    if (os.check_key(ctx) != 1) {
        os.ctx_free(ctx);
        snprintf(err, errn, "tls::identity: the key does not belong to the certificate");
        return NULL;
    }
    return ctx;
}

/* a client context that trusts only the given certificates */
static void *tls_trust_load(const char *pem, size_t n, char *err, size_t errn) {
    if (!tls_load() || !os_extra()) {
        snprintf(err, errn, "TLS is not available: install OpenSSL 3 (libssl.so.3); static builds cannot load it");
        return NULL;
    }
    void *ctx = os.ctx_new(os.method());
    void *bio = ctx ? os.bio_mem(pem, (int)n) : NULL;
    int count = 0;
    void *x;
    while (bio && (x = os.read_x509(bio, NULL, NULL, NULL)) != NULL) {
        os.store_add(os.cert_store(ctx), x);
        os.x509_free(x);
        count++;
    }
    if (bio) os.bio_free(bio);
    if (!count) {
        if (ctx) os.ctx_free(ctx);
        snprintf(err, errn, "no certificate found in the trusted certificate text (expected PEM)");
        return NULL;
    }
    os.set_verify(ctx, 1 /* SSL_VERIFY_PEER */, NULL);
    return ctx;
}

static void tls_trust_free(void *t) {
    if (t) os.ctx_free(t);
}

/* the server side of a TLS session on an accepted socket (the handshake
 * blocks this thread) */
static void *tls_accept_on(int fd, void *identity, char *err, size_t errn) {
    void *ssl = os.ssl_new(identity);
    if (!ssl) {
        snprintf(err, errn, "TLS: cannot create a session");
        return NULL;
    }
    os.set_fd(ssl, fd);
    if (os.accept(ssl) != 1) {
        snprintf(err, errn, "TLS with the client failed");
        os.free(ssl);
        return NULL;
    }
    return ssl;
}

static void *tls_open_trusting(int fd, const char *host, void *trust, char *err, size_t errn);
static void *tls_open(int fd, const char *host, char *err, size_t errn) { return tls_open_trusting(fd, host, NULL, err, errn); }

static void *tls_open_trusting(int fd, const char *host, void *trust, char *err, size_t errn) {
    if (!tls_load()) {
        snprintf(err, errn, "TLS is not available: install OpenSSL 3 (libssl.so.3); static builds cannot load it");
        return NULL;
    }
    void *ssl = os.ssl_new(trust ? trust : os.ctx);
    if (!ssl) {
        snprintf(err, errn, "TLS: cannot create a session");
        return NULL;
    }
    os.set_fd(ssl, fd);
    os.ctrl(ssl, 55 /* SSL_CTRL_SET_TLSEXT_HOSTNAME */, 0, (void *)host); /* SNI */
    os.set1_host(ssl, host);                                                /* check the name */
    if (os.connect(ssl) != 1) {
        long v = os.verify_result(ssl);
        if (v != 0 && os.verify_string) snprintf(err, errn, "TLS with %s failed: %s", host, os.verify_string(v));
        else snprintf(err, errn, "TLS with %s failed", host);
        os.free(ssl);
        return NULL;
    }
    return ssl;
}

static ssize_t tls_read(void *t, void *buf, size_t n) {
#if MIM_GREEN
    /* nothing decrypted yet: wait on the event loop rather than in SSL_read */
    if (t_co && os.pending && os.get_fd && os.pending(t) == 0) {
        int fd = os.get_fd(t);
        if (!co_wait_fd(fd, 0, sock_timeo(fd, SO_RCVTIMEO))) {
            errno = EAGAIN;
            return -1;
        }
    }
#endif
    int r = os.read(t, buf, (int)(n > 1 << 30 ? 1 << 30 : n));
    if (r > 0) return r;
    return 0; /* closed (or an error: treated as the end) */
}

static int tls_write_all(void *t, const char *p, size_t n) {
    while (n > 0) {
        int w = os.write(t, p, (int)(n > 1 << 30 ? 1 << 30 : n));
        if (w <= 0) return -1;
        p += w;
        n -= (size_t)w;
    }
    return 0;
}

static void tls_close(void *t) {
    os.shutdown(t);
    os.free(t);
}

#else

static void *tls_open(int fd, const char *host, char *err, size_t errn) {
    (void)fd;
    (void)host;
    snprintf(err, errn, "TLS is not available on this platform yet");
    return NULL;
}
static void *tls_open_trusting(int fd, const char *host, void *trust, char *err, size_t errn) {
    (void)trust;
    return tls_open(fd, host, err, errn);
}
static void *tls_identity_load(const char *c, size_t cn, const char *k, size_t kn, char *err, size_t errn) {
    (void)c; (void)cn; (void)k; (void)kn;
    snprintf(err, errn, "TLS is not available on this platform yet");
    return NULL;
}
static void *tls_trust_load(const char *p, size_t n, char *err, size_t errn) {
    (void)p; (void)n;
    snprintf(err, errn, "TLS is not available on this platform yet");
    return NULL;
}
static void tls_trust_free(void *t) { (void)t; }
static void *tls_accept_on(int fd, void *id, char *err, size_t errn) {
    (void)fd; (void)id;
    snprintf(err, errn, "TLS is not available on this platform yet");
    return NULL;
}
static ssize_t tls_read(void *t, void *buf, size_t n) { (void)t; (void)buf; (void)n; return -1; }
static int tls_write_all(void *t, const char *p, size_t n) { (void)t; (void)p; (void)n; return -1; }
static void tls_close(void *t) { (void)t; }

#endif

/* ---- P-256 keys, for tls::self_signed ------------------------------------
 * ec_keygen makes a key (public point 04|X|Y, private scalar) and ec_sign
 * makes a DER ECDSA signature over SHA-256 of a message. */

#if defined(__APPLE__)

static struct {
    int ready;
    const void *(*dict_create)(const void *alloc, const void **keys, const void **values, long n, const void *kcb, const void *vcb);
    const void *kcb, *vcb;
    const void *(*number_create)(const void *alloc, long type, const void *value);
    const uint8_t *(*data_ptr)(const void *d);
    long (*data_len)(const void *d);
    const void *(*random_key)(const void *attrs, const void **error);
    const void *(*export_key)(const void *key, const void **error);
    const void *(*sign)(const void *key, const void *alg, const void *data, const void **error);
    const void *const *attr_type, *const *attr_bits, *const *type_ec, *const *alg_sha256;
} sk;

static int sk_load(void) {
    if (!sx_load()) return 0;
    pthread_mutex_lock(&st_lock);
    if (!sk.ready) {
        void *sec = dlopen("/System/Library/Frameworks/Security.framework/Security", RTLD_LAZY | RTLD_LOCAL);
        void *cf = dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", RTLD_LAZY | RTLD_LOCAL);
        if (sec && cf) {
            *(void **)&sk.dict_create = dlsym(cf, "CFDictionaryCreate");
            sk.kcb = dlsym(cf, "kCFTypeDictionaryKeyCallBacks");
            sk.vcb = dlsym(cf, "kCFTypeDictionaryValueCallBacks");
            *(void **)&sk.number_create = dlsym(cf, "CFNumberCreate");
            *(void **)&sk.data_ptr = dlsym(cf, "CFDataGetBytePtr");
            *(void **)&sk.data_len = dlsym(cf, "CFDataGetLength");
            *(void **)&sk.random_key = dlsym(sec, "SecKeyCreateRandomKey");
            *(void **)&sk.export_key = dlsym(sec, "SecKeyCopyExternalRepresentation");
            *(void **)&sk.sign = dlsym(sec, "SecKeyCreateSignature");
            sk.attr_type = (const void *const *)dlsym(sec, "kSecAttrKeyType");
            sk.attr_bits = (const void *const *)dlsym(sec, "kSecAttrKeySizeInBits");
            sk.type_ec = (const void *const *)dlsym(sec, "kSecAttrKeyTypeECSECPrimeRandom");
            sk.alg_sha256 = (const void *const *)dlsym(sec, "kSecKeyAlgorithmECDSASignatureMessageX962SHA256");
        }
        sk.ready = sk.dict_create && sk.kcb && sk.vcb && sk.number_create && sk.data_ptr && sk.data_len && sk.random_key && sk.export_key &&
                           sk.sign && sk.attr_type && sk.attr_bits && sk.type_ec && sk.alg_sha256
                       ? 1
                       : -1;
    }
    pthread_mutex_unlock(&st_lock);
    return sk.ready == 1;
}

static void *ec_keygen(uint8_t pub[65], uint8_t priv[32], char *err, size_t errn) {
    if (!sk_load()) {
        snprintf(err, errn, "tls::self_signed: the Security framework could not be loaded");
        return NULL;
    }
    int32_t bits = 256;
    const void *num = sk.number_create(NULL, 3 /* kCFNumberSInt32Type */, &bits);
    const void *keys[2] = {*sk.attr_type, *sk.attr_bits}, *vals[2] = {*sk.type_ec, num};
    const void *attrs = sk.dict_create(NULL, keys, vals, 2, sk.kcb, sk.vcb);
    const void *key = sk.random_key(attrs, NULL);
    st.release(attrs);
    st.release(num);
    const void *ext = key ? sk.export_key(key, NULL) : NULL;
    if (!ext || sk.data_len(ext) != 97) {
        if (ext) st.release(ext);
        if (key) st.release(key);
        snprintf(err, errn, "tls::self_signed: cannot make a key");
        return NULL;
    }
    memcpy(pub, sk.data_ptr(ext), 65);
    memcpy(priv, sk.data_ptr(ext) + 65, 32);
    st.release(ext);
    return (void *)key;
}

static int ec_sign(void *key, const uint8_t *msg, size_t n, dbuf *sig) {
    const void *data = sx.data_create(NULL, msg, (long)n);
    const void *s = sk.sign(key, *sk.alg_sha256, data, NULL);
    st.release(data);
    if (!s) return 0;
    db_put(sig, sk.data_ptr(s), (size_t)sk.data_len(s));
    st.release(s);
    return 1;
}

static void ec_free(void *key) { st.release(key); }

#elif defined(__linux__)

static struct {
    int ready;
    void *(*keygen)(void *libctx, const char *propq, const char *type, ...);
    int (*get_octets)(const void *pkey, const char *name, unsigned char *buf, size_t max, size_t *len);
    int (*get_bn)(const void *pkey, const char *name, void **bn);
    int (*bn_bin)(const void *bn, unsigned char *to, int len);
    void (*bn_free)(void *bn);
    void *(*md_new)(void);
    void (*md_free)(void *ctx);
    const void *(*sha256)(void);
    int (*sign_init)(void *ctx, void **pctx, const void *md, void *engine, void *pkey);
    int (*sign)(void *ctx, unsigned char *sig, size_t *siglen, const unsigned char *tbs, size_t n);
    void (*pkey_free)(void *pkey);
} ek;

static int ek_load(void) {
    pthread_mutex_lock(&os_lock);
    if (!ek.ready) {
        void *c = dlopen("libcrypto.so.3", RTLD_LAZY | RTLD_GLOBAL);
        if (c) {
            *(void **)&ek.keygen = dlsym(c, "EVP_PKEY_Q_keygen");
            *(void **)&ek.get_octets = dlsym(c, "EVP_PKEY_get_octet_string_param");
            *(void **)&ek.get_bn = dlsym(c, "EVP_PKEY_get_bn_param");
            *(void **)&ek.bn_bin = dlsym(c, "BN_bn2binpad");
            *(void **)&ek.bn_free = dlsym(c, "BN_clear_free");
            *(void **)&ek.md_new = dlsym(c, "EVP_MD_CTX_new");
            *(void **)&ek.md_free = dlsym(c, "EVP_MD_CTX_free");
            *(void **)&ek.sha256 = dlsym(c, "EVP_sha256");
            *(void **)&ek.sign_init = dlsym(c, "EVP_DigestSignInit");
            *(void **)&ek.sign = dlsym(c, "EVP_DigestSign");
            *(void **)&ek.pkey_free = dlsym(c, "EVP_PKEY_free");
        }
        ek.ready = ek.keygen && ek.get_octets && ek.get_bn && ek.bn_bin && ek.bn_free && ek.md_new && ek.md_free && ek.sha256 &&
                           ek.sign_init && ek.sign && ek.pkey_free
                       ? 1
                       : -1;
    }
    pthread_mutex_unlock(&os_lock);
    return ek.ready == 1;
}

static void *ec_keygen(uint8_t pub[65], uint8_t priv[32], char *err, size_t errn) {
    if (!ek_load()) {
        snprintf(err, errn, "tls::self_signed: install OpenSSL 3 (libcrypto.so.3); static builds cannot load it");
        return NULL;
    }
    void *pk = ek.keygen(NULL, NULL, "EC", "P-256");
    size_t len = 0;
    void *bn = NULL;
    if (!pk || ek.get_octets(pk, "pub", pub, 65, &len) != 1 || len != 65 || ek.get_bn(pk, "priv", &bn) != 1 || ek.bn_bin(bn, priv, 32) != 32) {
        if (bn) ek.bn_free(bn);
        if (pk) ek.pkey_free(pk);
        snprintf(err, errn, "tls::self_signed: cannot make a key");
        return NULL;
    }
    ek.bn_free(bn);
    return pk;
}

static int ec_sign(void *key, const uint8_t *msg, size_t n, dbuf *sig) {
    void *ctx = ek.md_new();
    unsigned char buf[160];
    size_t len = sizeof buf;
    int ok = ctx && ek.sign_init(ctx, NULL, ek.sha256(), NULL, key) == 1 && ek.sign(ctx, buf, &len, msg, n) == 1;
    if (ctx) ek.md_free(ctx);
    if (ok) db_put(sig, buf, len);
    return ok;
}

static void ec_free(void *key) { ek.pkey_free(key); }

#else

static void *ec_keygen(uint8_t pub[65], uint8_t priv[32], char *err, size_t errn) {
    (void)pub; (void)priv;
    snprintf(err, errn, "TLS is not available on this platform yet");
    return NULL;
}
static int ec_sign(void *key, const uint8_t *msg, size_t n, dbuf *sig) { (void)key; (void)msg; (void)n; (void)sig; return 0; }
static void ec_free(void *key) { (void)key; }

#endif

/* ---- HTTP: shared parsing ----------------------------------------------- */

/* read until the end of the headers; returns the offset just past "\r\n\r\n"
 * (or -1), leaving any body bytes already read in `b` */
static int64_t read_head(mconn *c, sbuf *b) {
    char tmp[16384];
    for (;;) {
        for (size_t i = 0; i + 3 < b->n; i++)
            if (b->p[i] == '\r' && b->p[i + 1] == '\n' && b->p[i + 2] == '\r' && b->p[i + 3] == '\n') {
                b->p[b->n] = 0; /* text functions stop here (sb_put keeps a byte for it) */
                return (int64_t)i + 4;
            }
        if (b->n > (1 << 20)) return -1; /* headers over 1 MB */
        ssize_t r = conn_recv(c, tmp, sizeof tmp);
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
static mim_val *read_body(mconn *c, sbuf *b, int64_t off, int64_t content_length, int chunked, int to_eof) {
    char tmp[65536];
    if (content_length >= 0) {
        while ((int64_t)b->n - off < content_length) {
            ssize_t r = conn_recv(c, tmp, sizeof tmp);
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
            ssize_t r = conn_recv(c, tmp, sizeof tmp);
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
            ssize_t r = conn_recv(c, tmp, sizeof tmp);
            if (r < 0 && errno == EINTR) continue;
            if (r <= 0) break;
            sb_put(b, tmp, (size_t)r);
        }
    }
    return mim_str(b->p + off, (int64_t)b->n - off);
}

/* ---- HTTP client -------------------------------------------------------- */

static mim_val *http_do(const char *m, const char *u, mim_val *headers, mim_val *body, int hops);

mim_val *mim_std_http_request(mim_val *method, mim_val *url, mim_val *headers, mim_val *body) {
    return http_do(io_path(method, "request"), io_path(url, "request"), headers, body, 0);
}

/* one request; redirects (301 302 303 307 308) are followed, up to 10 */
static mim_val *http_do(const char *m, const char *u, mim_val *headers, mim_val *body, int hops) {
    int secure = strncmp(u, "https://", 8) == 0;
    if (!secure && strncmp(u, "http://", 7) != 0) return err_of("request: the URL must start with http:// or https://");
    const char *hp = u + (secure ? 8 : 7), *slash = strchr(hp, '/');
    size_t hl = slash ? (size_t)(slash - hp) : strlen(hp);
    char host[256];
    if (hl == 0 || hl >= sizeof host) return err_of("request: bad host in URL");
    memcpy(host, hp, hl);
    host[hl] = 0;
    int64_t port = secure ? 443 : 80;
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
    mconn conn = {fd, NULL, NULL};
    if (secure) {
        conn.tls = tls_open(fd, host, err, sizeof err);
        if (!conn.tls) {
            close(fd);
            return err_of(err);
        }
    }
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
    int bad = conn_send_all(&conn, q.p, q.n) != 0;
    free(q.p);
    if (bad) {
        mim_val *e = err_errno("cannot send the request to", host);
        if (conn.tls) tls_close(conn.tls);
        close(fd);
        return e;
    }
    sbuf b = {0};
    int64_t off = read_head(&conn, &b);
    if (off < 0) {
        free(b.p);
        if (conn.tls) tls_close(conn.tls);
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
    mim_val *bodyv = no_body ? mim_str("", 0) : read_body(&conn, &b, off, clen, chunked, 1);
    if (conn.tls) tls_close(conn.tls);
    close(fd);
    free(b.p);
    if (status == 301 || status == 302 || status == 303 || status == 307 || status == 308) {
        mim_val *key = mim_str("location", 8);
        int64_t at = map_find(hm, key, NULL);
        mim_release(key);
        if (at >= 0) {
            if (hops >= 10) {
                mim_release(hm);
                mim_release(bodyv);
                return err_of("too many redirects");
            }
            mim_val *loc = ((mim_val **)hm->data)[2 * at + 1];
            /* an absolute URL, or a path on the same server */
            char next[2048];
            const char *l = (const char *)loc->data;
            if (strncmp(l, "http://", 7) == 0 || strncmp(l, "https://", 8) == 0) snprintf(next, sizeof next, "%s", l);
            else snprintf(next, sizeof next, "%s%.*s%s%s", secure ? "https://" : "http://", (int)hl, hp, l[0] == '/' ? "" : "/", l);
            /* 303 (and the old habit for 301/302 after POST): the next request is a GET */
            int keep = status == 307 || status == 308;
            const char *nm = keep ? m : (strcmp(m, "HEAD") == 0 ? "HEAD" : "GET");
            mim_val *empty = mim_str("", 0);
            mim_val *r = http_do(nm, next, headers, keep ? body : empty, hops + 1);
            mim_release(empty);
            mim_release(hm);
            mim_release(bodyv);
            return r;
        }
    }
    mim_val *fields[3] = {mim_int(status), hm, bodyv};
    return ok_of(obj_owned(&mim_desc_http_Response, 3, fields));
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

/* answer one request whose head is in b; 1 when the connection stays open
 * for the next (b then holds what was read beyond this request) */
static int http_answer(mconn *c, mim_val *handler, sbuf *bp, int64_t off, int keep_ok, sbuf *pend);

/* one request on a connection: parse, call the handler, answer, close */
static void serve_one(mconn *c, mim_val *handler) {
    sock_timeout(c->fd, 30);
    sbuf b = {0};
    int64_t off = read_head(c, &b);
    if (off < 0) {
        free(b.p);
        conn_close(c);
        return;
    }
    http_answer(c, handler, &b, off, 0, NULL);
    free(b.p);
}

/* is a whole request head already read? */
static int head_ready(const sbuf *b) { return b->n >= 4 && memmem(b->p, b->n, "\r\n\r\n", 4) != NULL; }

/* HTTP/1.1 persistent connections: requests one after another on the same
 * connection until the client closes it, asks to, or is idle for 30 s */
static void serve_keep(mconn *c, mim_val *handler) {
    sock_timeout(c->fd, 30);
    sbuf b = {0}, pend = {0}; /* answers not sent yet */
    for (;;) {
        int64_t off = read_head(c, &b);
        if (off < 0) {
            if (pend.n) conn_send_all(c, pend.p, pend.n);
            conn_close(c);
            break;
        }
        if (!http_answer(c, handler, &b, off, 1, &pend)) break;
        /* pipelined requests: their answers leave together, in one write,
         * when the next would wait for the client (or 64 KB are waiting) */
        if (head_ready(&b) && pend.n < 65536) continue;
        if (conn_send_all(c, pend.p, pend.n) != 0) {
            conn_close(c);
            break;
        }
        pend.n = 0;
    }
    free(b.p);
    free(pend.p);
}

static int http_answer(mconn *c, mim_val *handler, sbuf *bp, int64_t off, int keep_ok, sbuf *pend) {
    sbuf b = *bp;
    /* the request line: within this request's head (the buffer may hold
     * the start of the next one after it) */
    const char *line_end = memmem(b.p, (size_t)off, "\r\n", 2);
    const char *sp1 = line_end ? memchr(b.p, ' ', (size_t)(line_end - b.p)) : NULL;
    const char *sp2 = sp1 ? memchr(sp1 + 1, ' ', (size_t)(line_end - sp1 - 1)) : NULL;
    if (!sp1 || !sp2) {
        const char *bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        if (pend && pend->n) conn_send_all(c, pend->p, pend->n);
        conn_send_all(c, bad, strlen(bad));
        conn_close(c);
        return 0;
    }
    const char *target = sp1 + 1, *q = memchr(target, '?', (size_t)(sp2 - target));
    int64_t clen;
    int chunked;
    mim_val *hm = parse_headers(line_end + 2, b.p + off - 2, &clen, &chunked);
    /* keep the connection: HTTP/1.1, no `Connection: close`, a body of known length */
    int keep = keep_ok && !chunked && (size_t)(line_end - sp2) == 9 && memcmp(sp2 + 1, "HTTP/1.1", 8) == 0;
    if (keep) {
        mim_val **h = (mim_val **)hm->data;
        int64_t count = ((const int64_t *)(const void *)hm->desc)[0];
        for (int64_t i = 0; i < count; i++) {
            mim_val *k = h[2 * i], *v = h[2 * i + 1];
            if (k->len == 10 && memcmp(k->data, "connection", 10) == 0 && v->len >= 5 && ieq((const char *)v->data, "close", 5)) keep = 0;
        }
    }
    int64_t body_len = clen < 0 && !chunked ? 0 : clen;
    /* the request line as offsets: reading the body may move the buffer */
    size_t m_end = (size_t)(sp1 - b.p), t_at = (size_t)(target - b.p), t_end = (size_t)((q ? q : sp2) - b.p), q_end = (size_t)(sp2 - b.p);
    int has_q = q != NULL;
    mim_val *bodyv = read_body(c, &b, off, body_len, chunked, 0);
    *bp = b; /* read_body may have grown (and moved) it */
    mim_val *fields[5] = {
        mim_str(b.p, (int64_t)m_end),
        mim_str(b.p + t_at, (int64_t)(t_end - t_at)),
        has_q ? mim_str(b.p + t_end + 1, (int64_t)(q_end - t_end - 1)) : mim_str("", 0),
        hm,
        bodyv,
    };
    mim_val *req = obj_owned(&mim_desc_http_Request, 5, fields);
    /* what the client sent after this request stays for the next */
    int64_t used = off + (body_len > 0 ? body_len : 0);
    if (used > (int64_t)b.n) used = (int64_t)b.n;
    memmove(b.p, b.p + used, b.n - (size_t)used);
    b.n -= (size_t)used;
    *bp = b;
    if (c->turn) co_lock(c->turn);
    mim_val *resp = mim_call(handler, 1, &req);
    if (c->turn) pthread_mutex_unlock(c->turn);
    mim_release(req);
    int64_t status = geti(field_of(resp, &mim_desc_http_Response, 0, "an HTTP handler's result"), 0);
    mim_val *rh = field_of(resp, &mim_desc_http_Response, 1, "an HTTP handler's result");
    mim_val *rb = field_of(resp, &mim_desc_http_Response, 2, "an HTTP handler's result");
    sbuf out = {0};
    sb_put(&out, "HTTP/1.1 ", 9);
    sb_i64(&out, status);
    sb_put(&out, " ", 1);
    sb_str(&out, reason(status));
    sb_put(&out, "\r\n", 2);
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
    sb_put(&out, "Content-Length: ", 16);
    sb_i64(&out, blen);
    sb_str(&out, keep ? "\r\n\r\n" : "\r\nConnection: close\r\n\r\n");
    if (blen) sb_put(&out, (const char *)rb->data, (size_t)blen);
    mim_release(resp);
    if (pend) { /* after the answers before it */
        sb_put(pend, out.p, out.n);
        free(out.p);
        if (keep) return 1;
        out = *pend;
        *pend = (sbuf){0};
    }
    int sent = conn_send_all(c, out.p, out.n);
    free(out.p);
    if (keep && sent == 0) return 1;
    if (!c->tls) shutdown(c->fd, SHUT_WR);
    conn_close(c);
    return 0;
}

typedef void (*conn_fn)(mconn *c, mim_val *handler);

typedef struct {
    int fd;
    mim_val *handler;
    conn_fn one;
    void *identity; /* a TLS server identity, or NULL */
    pthread_mutex_t *turn;
} conn_job;

/* serve one accepted connection: the TLS handshake first, if any */
static void conn_run(int fd, mim_val *handler, conn_fn one, void *identity, pthread_mutex_t *turn) {
    mconn c = {fd, NULL, turn};
    if (identity) {
        sock_timeout(fd, 30);
        char err[256];
        c.tls = tls_accept_on(fd, identity, err, sizeof err);
        if (!c.tls) { /* not a TLS client, or it gave up */
            close(fd);
            return;
        }
    }
    one(&c, handler);
}

static void *conn_main(void *p) {
    conn_job *j = (conn_job *)p;
    conn_run(j->fd, j->handler, j->one, j->identity, j->turn);
    mim_release(j->handler);
    free(j);
    free_list_drain(); /* this thread ends now */
    return NULL;
}

#if MIM_GREEN
static void conn_co_main(void *p) {
    conn_job *j = (conn_job *)p;
    conn_run(j->fd, j->handler, j->one, j->identity, j->turn);
    mim_release(j->handler);
    free(j);
}
#endif

/* accept `limit` connections (0 = forever); handlers whose effects may
 * overlap get a task each (a coroutine on the event loop, else a thread),
 * others run one at a time in order */
static mim_val *serve_loop_with(mim_val *port, mim_val *handler, int64_t limit, conn_fn one, void *identity);

static mim_val *serve_loop(mim_val *port, mim_val *handler, int64_t limit) { return serve_loop_with(port, handler, limit, serve_one, NULL); }

/* accept `limit` connections (0 = forever) and run `one` on each */
static mim_val *serve_loop_ex(mim_val *port, mim_val *handler, int64_t limit, conn_fn one, void *identity, int parallel);

static mim_val *serve_loop_with(mim_val *port, mim_val *handler, int64_t limit, conn_fn one, void *identity) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("serve: the handler must be a function");
    return serve_loop_ex(port, handler, limit, one, identity, MIM_THREADS && FN_DESC(handler)->pure);
}

/* accept connections and run `one` on each (a task each when `parallel`);
 * `limit` > 0 stops after that many */
static mim_val *serve_loop_ex(mim_val *port, mim_val *handler, int64_t limit, conn_fn one, void *identity, int parallel) {
    mim_val *lr = mim_std_tcp_listen(port);
    if (lr->desc != &mim_desc_Ok) return lr;
    mim_val *l = ((mim_val **)lr->data)[0];
    int lfd = (int)geti(((mim_val **)l->data)[0], 0);
    mim_release(lr);
    int64_t served = 0;
    /* every connection is a task (so one slow or idle client cannot hold up
     * the others); handlers with effects take turns through `turn` */
    pthread_mutex_t *turn = NULL;
    if (!parallel) { /* kept for good when serving forever: tasks may outlive the loop */
        turn = (pthread_mutex_t *)malloc(sizeof(pthread_mutex_t));
        pthread_mutex_init(turn, NULL);
    }
    parallel = MIM_THREADS;
#if MIM_GREEN
    int green = parallel && green_on();
    mim_co **tasks = green && limit > 0 ? (mim_co **)calloc((size_t)limit, sizeof(mim_co *)) : NULL;
#else
    int green = 0;
#endif
    pthread_t *threads = limit > 0 && parallel && !green ? (pthread_t *)calloc((size_t)limit, sizeof(pthread_t)) : NULL;
    while (limit == 0 || served < limit) {
        mim_wait_readable(lfd);
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
            j->one = one;
            j->identity = identity;
            j->turn = turn;
            mim_retain(handler);
#if MIM_GREEN
            if (green) {
                mim_co *co = co_spawn(conn_co_main, j, tasks == NULL);
                if (tasks) tasks[served] = co;
                served++;
                continue;
            }
#endif
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
            conn_run(fd, handler, one, identity, turn);
        }
        served++;
    }
#if MIM_GREEN
    if (tasks) {
        for (int64_t i = 0; i < served; i++) co_join(tasks[i]);
        free(tasks);
    }
#endif
    (void)green;
    if (threads) {
        for (int64_t i = 0; i < served; i++)
            if (threads[i]) pthread_join(threads[i], NULL);
        free(threads);
    }
    if (turn && limit > 0) { /* every task has been joined */
        pthread_mutex_destroy(turn);
        free(turn);
    }
    close(lfd);
    return ok_of(mim_int(served));
}

mim_val *mim_std_http_serve(mim_val *port, mim_val *handler) { return serve_loop_with(port, handler, 0, serve_keep, NULL); }

mim_val *mim_std_http_serve_requests(mim_val *port, mim_val *handler, mim_val *count) {
    return serve_loop(port, handler, geti(count, 0) > 0 ? geti(count, 0) : 1);
}

/* ==== WebSockets (RFC 6455) ================================================ */

extern const mim_desc mim_desc_ws_Socket, mim_desc_ws_Text, mim_desc_ws_Binary, mim_desc_ws_Closed;

/* SHA-1, for the handshake's Sec-WebSocket-Accept only */
static void sha1(const uint8_t *msg, size_t len, uint8_t out[20]) {
    uint32_t h[5] = {0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0};
    size_t total = ((len + 8) / 64 + 1) * 64;
    uint8_t *m = (uint8_t *)calloc(total, 1);
    memcpy(m, msg, len);
    m[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int k = 0; k < 8; k++) m[total - 1 - k] = (uint8_t)(bits >> (8 * k));
    for (size_t off = 0; off < total; off += 64) {
        uint32_t w[80];
        for (int t = 0; t < 16; t++)
            w[t] = (uint32_t)m[off + 4 * t] << 24 | (uint32_t)m[off + 4 * t + 1] << 16 | (uint32_t)m[off + 4 * t + 2] << 8 | m[off + 4 * t + 3];
        for (int t = 16; t < 80; t++) {
            uint32_t x = w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16];
            w[t] = x << 1 | x >> 31;
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4];
        for (int t = 0; t < 80; t++) {
            uint32_t f, k;
            if (t < 20) { f = (b & c) | (~b & d); k = 0x5A827999; }
            else if (t < 40) { f = b ^ c ^ d; k = 0x6ED9EBA1; }
            else if (t < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8F1BBCDC; }
            else { f = b ^ c ^ d; k = 0xCA62C1D6; }
            uint32_t tmp = (a << 5 | a >> 27) + f + e + k + w[t];
            e = d; d = c; c = b << 30 | b >> 2; b = a; a = tmp;
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e;
    }
    free(m);
    for (int k = 0; k < 5; k++)
        for (int q = 0; q < 4; q++) out[4 * k + q] = (uint8_t)(h[k] >> (24 - 8 * q));
}

static void b64_encode(const uint8_t *p, size_t n, char *o) {
    size_t k = 0;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)p[i] << 16 | (i + 1 < n ? (uint32_t)p[i + 1] << 8 : 0) | (i + 2 < n ? p[i + 2] : 0);
        o[k++] = B64[v >> 18 & 63];
        o[k++] = B64[v >> 12 & 63];
        o[k++] = i + 1 < n ? B64[v >> 6 & 63] : '=';
        o[k++] = i + 2 < n ? B64[v & 63] : '=';
    }
    o[k] = 0;
}

/* the Sec-WebSocket-Accept value for a key */
static void ws_accept_of(const char *key, size_t klen, char out[32]) {
    static const char guid[] = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    char buf[128];
    if (klen > 60) klen = 60;
    memcpy(buf, key, klen);
    memcpy(buf + klen, guid, sizeof guid - 1);
    uint8_t d[20];
    sha1((const uint8_t *)buf, klen + sizeof guid - 1, d);
    b64_encode(d, 20, out);
}

typedef struct {
    mconn c;
    int client; /* clients mask what they send */
    int closed;
    pthread_mutex_t send_lock;
    char *pending; /* bytes read past the handshake */
    size_t pend_n, pend_off;
} ws_conn;

static ws_conn **ws_tab;
static int64_t ws_n, ws_cap;
static pthread_mutex_t ws_tab_lock = PTHREAD_MUTEX_INITIALIZER;

static mim_val *ws_socket_of(ws_conn *w) {
    pthread_mutex_lock(&ws_tab_lock);
    if (ws_n == ws_cap) {
        ws_cap = ws_cap ? ws_cap * 2 : 16;
        ws_tab = (ws_conn **)realloc(ws_tab, sizeof(ws_conn *) * (size_t)ws_cap);
    }
    int64_t id = ws_n++;
    ws_tab[id] = w;
    pthread_mutex_unlock(&ws_tab_lock);
    mim_val *f = mim_int(id);
    return obj_owned(&mim_desc_ws_Socket, 1, &f);
}

static ws_conn *ws_get(mim_val *sock, const char *what) {
    if (!sock || sock->dtype != MIM_OBJ || sock->desc != &mim_desc_ws_Socket) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected a ws::Socket", what);
        mim_panic(buf);
    }
    int64_t id = geti(((mim_val **)sock->data)[0], 0);
    pthread_mutex_lock(&ws_tab_lock);
    ws_conn *w = id >= 0 && id < ws_n ? ws_tab[id] : NULL;
    pthread_mutex_unlock(&ws_tab_lock);
    return w;
}

static ws_conn *ws_new(int fd, void *tls, int client, const char *rest, size_t rest_n) {
    ws_conn *w = (ws_conn *)calloc(1, sizeof(ws_conn));
    w->c.fd = fd;
    w->c.tls = tls;
    w->client = client;
    pthread_mutex_init(&w->send_lock, NULL);
    if (rest_n) {
        w->pending = (char *)malloc(rest_n);
        memcpy(w->pending, rest, rest_n);
        w->pend_n = rest_n;
    }
    return w;
}

static int ws_read_exact(ws_conn *w, void *buf, size_t n) {
    size_t got = 0;
    if (w->pend_off < w->pend_n) {
        size_t k = w->pend_n - w->pend_off < n ? w->pend_n - w->pend_off : n;
        memcpy(buf, w->pending + w->pend_off, k);
        w->pend_off += k;
        got = k;
    }
    while (got < n) {
        ssize_t r = conn_recv(&w->c, (char *)buf + got, n - got);
        if (r < 0 && errno == EINTR) continue;
        if (r <= 0) return -1;
        got += (size_t)r;
    }
    return 0;
}

static int ws_write_frame(ws_conn *w, int op, const void *data, size_t n) {
    uint8_t hdr[14];
    size_t h = 0;
    hdr[h++] = (uint8_t)(0x80 | op);
    uint8_t maskbit = w->client ? 0x80 : 0;
    if (n < 126) {
        hdr[h++] = (uint8_t)(maskbit | n);
    } else if (n < 65536) {
        hdr[h++] = maskbit | 126;
        hdr[h++] = (uint8_t)(n >> 8);
        hdr[h++] = (uint8_t)n;
    } else {
        hdr[h++] = maskbit | 127;
        for (int k = 7; k >= 0; k--) hdr[h++] = (uint8_t)((uint64_t)n >> (8 * k));
    }
    char *frame = (char *)malloc(h + 4 + n + 1);
    uint8_t mask[4] = {0};
    if (w->client) {
        uint64_t r = rng_next();
        memcpy(mask, &r, 4);
        memcpy(hdr + h, mask, 4);
        h += 4;
    }
    memcpy(frame, hdr, h);
    for (size_t k = 0; k < n; k++) frame[h + k] = (char)(((const uint8_t *)data)[k] ^ (w->client ? mask[k & 3] : 0));
    co_lock(&w->send_lock); /* frames from different tasks never interleave */
    int rc = conn_send_all(&w->c, frame, h + n);
    pthread_mutex_unlock(&w->send_lock);
    free(frame);
    return rc;
}

static void ws_free(ws_conn *w) {
    if (w->c.tls) tls_close(w->c.tls);
    close(w->c.fd);
    pthread_mutex_destroy(&w->send_lock);
    free(w->pending);
    free(w);
}

/* forget a connection's table slot (ids are never reused) and free it */
static void ws_drop(mim_val *sock) {
    int64_t id = geti(((mim_val **)sock->data)[0], 0);
    pthread_mutex_lock(&ws_tab_lock);
    ws_conn *w = id >= 0 && id < ws_n ? ws_tab[id] : NULL;
    if (w) ws_tab[id] = NULL;
    pthread_mutex_unlock(&ws_tab_lock);
    if (w) {
        if (!w->closed) {
            uint8_t code[2] = {0x03, 0xE8}; /* 1000: normal closure */
            ws_write_frame(w, 8, code, 2);
        }
        ws_free(w);
    }
}

/* ws://host[:port]/path or wss://… */
mim_val *mim_std_ws_connect(mim_val *url) {
    const char *u = io_path(url, "ws::connect");
    int secure = strncmp(u, "wss://", 6) == 0;
    if (!secure && strncmp(u, "ws://", 5) != 0) return err_of("ws::connect: the URL must start with ws:// or wss://");
    const char *hp = u + (secure ? 6 : 5), *slash = strchr(hp, '/');
    size_t hl = slash ? (size_t)(slash - hp) : strlen(hp);
    char host[256];
    if (hl == 0 || hl >= sizeof host) return err_of("ws::connect: bad host in URL");
    memcpy(host, hp, hl);
    host[hl] = 0;
    int64_t port = secure ? 443 : 80;
    char *colon = strrchr(host, ':');
    if (colon) {
        port = atoll(colon + 1);
        *colon = 0;
    }
    char err[512];
    int fd = dial(host, port, err, sizeof err);
    if (fd < 0) return err_of(err);
    sock_timeout(fd, 30);
    mconn c = {fd, NULL, NULL};
    if (secure && !(c.tls = tls_open(fd, host, err, sizeof err))) {
        close(fd);
        return err_of(err);
    }
    uint8_t raw[16];
    for (int k = 0; k < 16; k += 8) {
        uint64_t r = rng_next();
        memcpy(raw + k, &r, 8);
    }
    char key[32], want[32], req[1024];
    b64_encode(raw, 16, key);
    ws_accept_of(key, strlen(key), want);
    snprintf(req, sizeof req,
             "GET %s HTTP/1.1\r\nHost: %.*s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n",
             slash ? slash : "/", (int)hl, hp, key);
    sbuf b = {0};
    int64_t off = conn_send_all(&c, req, strlen(req)) == 0 ? read_head(&c, &b) : -1;
    int ok = 0;
    if (off > 0) {
        const char *sp = memchr(b.p, ' ', (size_t)off);
        int64_t clen;
        int chunked;
        const char *first_end = strstr(b.p, "\r\n");
        mim_val *hm = parse_headers(first_end + 2, b.p + off - 2, &clen, &chunked);
        mim_val *k = mim_str("sec-websocket-accept", 20);
        int64_t at = map_find(hm, k, NULL);
        mim_release(k);
        ok = sp && atoll(sp + 1) == 101 && at >= 0 && strcmp((const char *)((mim_val **)hm->data)[2 * at + 1]->data, want) == 0;
        mim_release(hm);
    }
    if (!ok) {
        free(b.p);
        if (c.tls) tls_close(c.tls);
        close(fd);
        return err_of(off < 0 ? "ws::connect: no answer to the WebSocket handshake" : "ws::connect: the server refused the WebSocket handshake");
    }
    sock_timeout(fd, 0); /* a WebSocket may stay quiet for a long time */
    ws_conn *w = ws_new(fd, c.tls, 1, b.p + off, b.n - (size_t)off);
    free(b.p);
    return ok_of(ws_socket_of(w));
}

static mim_val *ws_send_op(mim_val *sock, int op, const void *p, size_t n, const char *what) {
    ws_conn *w = ws_get(sock, what);
    if (!w || w->closed) return err_of("the WebSocket is closed");
    if (ws_write_frame(w, op, p, n) != 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int((int64_t)n));
}

mim_val *mim_std_ws_send(mim_val *sock, mim_val *text) {
    if (!text || text->dtype != MIM_CHAR || text->rank != 1) mim_panic("ws::send: expected text (str)");
    return ws_send_op(sock, 1, text->data, (size_t)text->len, "ws::send");
}

mim_val *mim_std_ws_send_bytes(mim_val *sock, mim_val *b) {
    if (!b || b->rank != 1 || (b->dtype != MIM_U8 && b->len != 0)) mim_panic("ws::send_bytes: expected bytes ([u8])");
    return ws_send_op(sock, 2, b->data, (size_t)b->len, "ws::send_bytes");
}

/* the next message: Text, Binary or Closed (pings are answered here) */
mim_val *mim_std_ws_receive(mim_val *sock) {
    ws_conn *w = ws_get(sock, "ws::receive");
    if (!w || w->closed) return ok_of(obj_owned(&mim_desc_ws_Closed, 0, NULL));
    sbuf msg = {0};
    int msg_op = -1;
    for (;;) {
        uint8_t h[2];
        if (ws_read_exact(w, h, 2) != 0) break;
        int fin = h[0] & 0x80, op = h[0] & 0x0F, masked = h[1] & 0x80;
        uint64_t n = h[1] & 0x7F;
        if (n == 126) {
            uint8_t e[2];
            if (ws_read_exact(w, e, 2) != 0) break;
            n = (uint64_t)e[0] << 8 | e[1];
        } else if (n == 127) {
            uint8_t e[8];
            if (ws_read_exact(w, e, 8) != 0) break;
            n = 0;
            for (int k = 0; k < 8; k++) n = n << 8 | e[k];
        }
        if (n > (64u << 20)) {
            free(msg.p);
            return err_of("WebSocket message over 64 MB");
        }
        uint8_t mask[4] = {0};
        if (masked && ws_read_exact(w, mask, 4) != 0) break;
        char *data = (char *)malloc(n ? n : 1);
        if (n && ws_read_exact(w, data, n) != 0) {
            free(data);
            break;
        }
        if (masked)
            for (uint64_t k = 0; k < n; k++) data[k] ^= (char)mask[k & 3];
        if (op == 9) { /* ping → pong */
            ws_write_frame(w, 10, data, n);
            free(data);
            continue;
        }
        if (op == 10) {
            free(data);
            continue;
        }
        if (op == 8) { /* close: answer it */
            if (!w->closed) ws_write_frame(w, 8, data, n >= 2 ? 2 : 0);
            w->closed = 1;
            free(data);
            free(msg.p);
            return ok_of(obj_owned(&mim_desc_ws_Closed, 0, NULL));
        }
        if (op == 1 || op == 2) msg_op = op;
        sb_put(&msg, data, n);
        free(data);
        if (fin && msg_op >= 0) {
            mim_val *v;
            if (msg_op == 1) {
                v = mim_str(msg.p ? msg.p : "", (int64_t)msg.n);
                v = obj_owned(&mim_desc_ws_Text, 1, &v);
            } else {
                mim_val *b = alloc(MIM_U8, 1, (int64_t)msg.n);
                if (msg.n) memcpy(b->data, msg.p, msg.n);
                v = obj_owned(&mim_desc_ws_Binary, 1, &b);
            }
            free(msg.p);
            return ok_of(v);
        }
    }
    free(msg.p);
    w->closed = 1;
    return err_of("the WebSocket connection was lost");
}

mim_val *mim_std_ws_close(mim_val *sock) {
    ws_get(sock, "ws::close");
    ws_drop(sock);
    return NULL;
}

/* one WebSocket connection on a server: handshake, the handler, closing */
static void ws_after_head(mconn *c, mim_val *handler, sbuf b, int64_t off);

static void ws_serve_one(mconn *c, mim_val *handler) {
    sock_timeout(c->fd, 30);
    sbuf b = {0};
    int64_t off = read_head(c, &b);
    if (off < 0) {
        free(b.p);
        conn_close(c);
        return;
    }
    ws_after_head(c, handler, b, off);
}

/* the WebSocket handshake and session for a request whose head is in b */
static void ws_after_head(mconn *c, mim_val *handler, sbuf b, int64_t off) {
    const char *first_end = strstr(b.p, "\r\n");
    int64_t clen;
    int chunked;
    mim_val *hm = parse_headers(first_end + 2, b.p + off - 2, &clen, &chunked);
    mim_val *k = mim_str("sec-websocket-key", 17);
    int64_t at = map_find(hm, k, NULL);
    mim_release(k);
    if (at < 0) {
        const char *bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 27\r\nConnection: close\r\n\r\nexpected a WebSocket client";
        conn_send_all(c, bad, strlen(bad));
        mim_release(hm);
        free(b.p);
        conn_close(c);
        return;
    }
    mim_val *key = ((mim_val **)hm->data)[2 * at + 1];
    char acc[32], resp[256];
    ws_accept_of((const char *)key->data, (size_t)key->len, acc);
    mim_release(hm);
    snprintf(resp, sizeof resp, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n", acc);
    if (conn_send_all(c, resp, strlen(resp)) != 0) {
        free(b.p);
        conn_close(c);
        return;
    }
    sock_timeout(c->fd, 0);
    ws_conn *w = ws_new(c->fd, c->tls, 0, b.p + off, b.n - (size_t)off);
    free(b.p);
    mim_val *sock = ws_socket_of(w);
    if (c->turn) co_lock(c->turn);
    mim_val *r = mim_call(handler, 1, &sock);
    if (c->turn) pthread_mutex_unlock(c->turn);
    if (r) mim_release(r);
    ws_drop(sock); /* closes it, if the handler did not */
    mim_release(sock);
}

mim_val *mim_std_ws_serve(mim_val *port, mim_val *handler) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve: the handler must be a function");
    return serve_loop_with(port, handler, 0, ws_serve_one, NULL);
}

mim_val *mim_std_ws_serve_connections(mim_val *port, mim_val *handler, mim_val *count) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve_connections: the handler must be a function");
    return serve_loop_with(port, handler, geti(count, 0) > 0 ? geti(count, 0) : 1, ws_serve_one, NULL);
}

/* ==== UDP ================================================================
 * Datagrams over IPv4: each send is one packet, each receive returns one
 * packet with the sender's address. Packets may be lost or reordered. */

extern const mim_desc mim_desc_udp_Socket, mim_desc_udp_Packet;

static int udp_fd(mim_val *s, const char *what) {
    return (int)geti(field_of(s, &mim_desc_udp_Socket, 0, what), 0);
}

/* bind a UDP socket on every interface; port 0 picks a free port (`.port`) */
mim_val *mim_std_udp_bind(mim_val *port) {
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) return err_errno("cannot open a UDP socket", NULL);
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &one, sizeof one);
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    a.sin_port = htons((uint16_t)geti(port, 0));
    if (bind(fd, (struct sockaddr *)&a, sizeof a) != 0) {
        char what[64];
        snprintf(what, sizeof what, "port %lld", (long long)geti(port, 0));
        mim_val *e = err_errno("cannot bind UDP", what);
        close(fd);
        return e;
    }
    socklen_t len = sizeof a;
    getsockname(fd, (struct sockaddr *)&a, &len);
    mim_val *f[2] = {mim_int(fd), mim_int(ntohs(a.sin_port))};
    return ok_of(obj_owned(&mim_desc_udp_Socket, 2, f));
}

static mim_val *udp_send_raw(mim_val *s, mim_val *host, mim_val *port, const char *p, size_t n, const char *what) {
    int fd = udp_fd(s, what);
    const char *h = io_path(host, what);
    char ps[16];
    snprintf(ps, sizeof ps, "%lld", (long long)geti(port, 0));
    struct addrinfo hints = {0}, *res = NULL;
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_DGRAM;
    int rc = getaddrinfo(h, ps, &hints, &res);
    if (rc != 0) {
        char buf[512];
        snprintf(buf, sizeof buf, "cannot resolve %s: %s", h, gai_strerror(rc));
        return err_of(buf);
    }
    ssize_t w = msendto(fd, p, n, res->ai_addr, res->ai_addrlen);
    freeaddrinfo(res);
    if (w < 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int((int64_t)w));
}

mim_val *mim_std_udp_send(mim_val *s, mim_val *host, mim_val *port, mim_val *text) {
    if (!text || text->dtype != MIM_CHAR || text->rank != 1) mim_panic("udp::send: expected text (str)");
    return udp_send_raw(s, host, port, (const char *)text->data, (size_t)text->len, "udp::send");
}

mim_val *mim_std_udp_send_bytes(mim_val *s, mim_val *host, mim_val *port, mim_val *b) {
    if (!b || b->rank != 1 || (b->dtype != MIM_U8 && b->len != 0)) mim_panic("udp::send_bytes: expected bytes ([u8])");
    return udp_send_raw(s, host, port, (const char *)b->data, (size_t)b->len, "udp::send_bytes");
}

/* one packet: Ok(Packet { data, text, host, port }) */
mim_val *mim_std_udp_receive(mim_val *s) {
    int fd = udp_fd(s, "udp::receive");
    char *buf = (char *)malloc(65536);
    struct sockaddr_in from;
    socklen_t fl = sizeof from;
    ssize_t r;
    while ((r = mrecvfrom(fd, buf, 65536, (struct sockaddr *)&from, &fl)) < 0 && errno == EINTR) {
    }
    if (r < 0) {
        free(buf);
        if (errno == EAGAIN || errno == EWOULDBLOCK) return err_of("timed out");
        return err_errno("cannot receive", NULL);
    }
    mim_val *data = alloc(MIM_U8, 1, (int64_t)r);
    if (r) memcpy(data->data, buf, (size_t)r);
    mim_val *text = mim_str(buf, (int64_t)r);
    free(buf);
    char host[INET_ADDRSTRLEN] = "";
    inet_ntop(AF_INET, &from.sin_addr, host, sizeof host);
    mim_val *f[4] = {data, text, mim_str(host, (int64_t)strlen(host)), mim_int(ntohs(from.sin_port))};
    return ok_of(obj_owned(&mim_desc_udp_Packet, 4, f));
}

/* like receive, but Err("timed out") after `timeout` (a duration) */
mim_val *mim_std_udp_receive_within(mim_val *s, mim_val *timeout) {
    int fd = udp_fd(s, "udp::receive_within");
    int64_t ns = timeout && timeout->rank == 0 ? (timeout->dtype == MIM_F64 ? (int64_t)getf(timeout, 0) : geti(timeout, 0)) : 0;
    if (ns < 1000) ns = 1000; /* 0 would mean "wait forever" to the socket */
    struct timeval tv = {(time_t)(ns / 1000000000), (int)(ns % 1000000000 / 1000)};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    mim_val *r = mim_std_udp_receive(s);
    struct timeval none = {0, 0};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &none, sizeof none);
    return r;
}

mim_val *mim_std_udp_close(mim_val *s) {
    close(udp_fd(s, "udp::close"));
    return NULL;
}

/* ==== TLS over TCP, and TLS servers =====================================
 * Identities (a certificate chain and its key) and streams live in tables;
 * the Mimic values only hold their index. Identities are kept for the whole
 * run (a server uses one for all its connections). */

extern const mim_desc mim_desc_tls_Identity, mim_desc_tls_Stream;

static void **tls_ids;
static int64_t tls_ids_n, tls_ids_cap;
static mconn **tls_streams;
static int64_t tls_streams_n, tls_streams_cap;
static pthread_mutex_t tls_tab_lock = PTHREAD_MUTEX_INITIALIZER;

static int64_t tab_add(void ***tab, int64_t *n, int64_t *cap, void *x) {
    pthread_mutex_lock(&tls_tab_lock);
    if (*n == *cap) {
        *cap = *cap ? *cap * 2 : 16;
        *tab = (void **)realloc(*tab, sizeof(void *) * (size_t)*cap);
    }
    int64_t id = (*n)++;
    (*tab)[id] = x;
    pthread_mutex_unlock(&tls_tab_lock);
    return id;
}

static void *identity_of(mim_val *v, const char *what) {
    int64_t id = geti(field_of(v, &mim_desc_tls_Identity, 0, what), 0);
    pthread_mutex_lock(&tls_tab_lock);
    void *x = id >= 0 && id < tls_ids_n ? tls_ids[id] : NULL;
    pthread_mutex_unlock(&tls_tab_lock);
    if (!x) mim_panic("not a TLS identity");
    return x;
}

static mconn *stream_of(mim_val *v, const char *what) {
    int64_t id = geti(field_of(v, &mim_desc_tls_Stream, 0, what), 0);
    pthread_mutex_lock(&tls_tab_lock);
    mconn *c = id >= 0 && id < tls_streams_n ? tls_streams[id] : NULL;
    pthread_mutex_unlock(&tls_tab_lock);
    return c;
}

static mim_val *text_arg(mim_val *t, const char *what) {
    if (!t || t->dtype != MIM_CHAR || t->rank != 1) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected text (str)", what);
        mim_panic(buf);
    }
    return t;
}

/* a server identity from a PEM certificate (chain) and its PEM private key */
mim_val *mim_std_tls_identity(mim_val *cert, mim_val *key) {
    text_arg(cert, "tls::identity");
    text_arg(key, "tls::identity");
    char err[512];
    void *x = tls_identity_load((const char *)cert->data, (size_t)cert->len, (const char *)key->data, (size_t)key->len, err, sizeof err);
    if (!x) return err_of(err);
    mim_val *f = mim_int(tab_add(&tls_ids, &tls_ids_n, &tls_ids_cap, x));
    return ok_of(obj_owned(&mim_desc_tls_Identity, 1, &f));
}

static mim_val *stream_val(int fd, void *tls) {
    mconn *c = (mconn *)calloc(1, sizeof(mconn));
    c->fd = fd;
    c->tls = tls;
    mim_val *f = mim_int(tab_add((void ***)&tls_streams, &tls_streams_n, &tls_streams_cap, c));
    return ok_of(obj_owned(&mim_desc_tls_Stream, 1, &f));
}

static mim_val *tls_dial(mim_val *host, mim_val *port, mim_val *pem, const char *what) {
    const char *h = io_path(host, what);
    char err[512];
    void *trust = NULL;
    if (pem) {
        text_arg(pem, what);
        trust = tls_trust_load((const char *)pem->data, (size_t)pem->len, err, sizeof err);
        if (!trust) return err_of(err);
    }
    int fd = dial(h, geti(port, 0), err, sizeof err);
    if (fd < 0) {
        tls_trust_free(trust);
        return err_of(err);
    }
    sock_timeout(fd, 30);
    void *t = tls_open_trusting(fd, h, trust, err, sizeof err);
    tls_trust_free(trust);
    if (!t) {
        close(fd);
        return err_of(err);
    }
    sock_timeout(fd, 0);
    return stream_val(fd, t);
}

mim_val *mim_std_tls_connect(mim_val *host, mim_val *port) { return tls_dial(host, port, NULL, "tls::connect"); }

mim_val *mim_std_tls_connect_trusting(mim_val *host, mim_val *port, mim_val *cert) { return tls_dial(host, port, cert, "tls::connect_trusting"); }

/* accept one connection on a tcp::Listener and do the server handshake */
mim_val *mim_std_tls_accept(mim_val *l, mim_val *identity) {
    void *id = identity_of(identity, "tls::accept");
    int lfd = fd_of(l, "tls::accept");
    int fd;
    mim_wait_readable(lfd);
    while ((fd = accept(lfd, NULL, NULL)) < 0 && errno == EINTR) {
    }
    if (fd < 0) return err_errno("cannot accept", NULL);
    sock_setup(fd);
    sock_timeout(fd, 30);
    char err[512];
    void *t = tls_accept_on(fd, id, err, sizeof err);
    if (!t) {
        close(fd);
        return err_of(err);
    }
    sock_timeout(fd, 0);
    return stream_val(fd, t);
}

static mim_val *stream_closed(void) { return err_of("the TLS stream is closed"); }

mim_val *mim_std_tls_send(mim_val *s, mim_val *text) {
    mconn *c = stream_of(s, "tls::send");
    text_arg(text, "tls::send");
    if (!c) return stream_closed();
    if (conn_send_all(c, (const char *)text->data, (size_t)text->len) != 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int(text->len));
}

mim_val *mim_std_tls_send_bytes(mim_val *s, mim_val *b) {
    mconn *c = stream_of(s, "tls::send_bytes");
    if (!b || b->rank != 1 || (b->dtype != MIM_U8 && b->len != 0)) mim_panic("tls::send_bytes: expected bytes ([u8])");
    if (!c) return stream_closed();
    if (conn_send_all(c, (const char *)b->data, (size_t)b->len) != 0) return err_errno("cannot send", NULL);
    return ok_of(mim_int(b->len));
}

/* up to `limit` bytes; "" when the other side has closed */
mim_val *mim_std_tls_receive(mim_val *s, mim_val *limit) {
    mconn *c = stream_of(s, "tls::receive");
    if (!c) return stream_closed();
    int64_t n = geti(limit, 0);
    if (n <= 0) n = 65536;
    char *buf = (char *)malloc((size_t)n);
    ssize_t r;
    while ((r = conn_recv(c, buf, (size_t)n)) < 0 && errno == EINTR) {
    }
    if (r < 0) {
        free(buf);
        return err_errno("cannot receive", NULL);
    }
    mim_val *v = mim_str(buf, (int64_t)r);
    free(buf);
    return ok_of(v);
}

mim_val *mim_std_tls_receive_bytes(mim_val *s, mim_val *limit) {
    mim_val *r = mim_std_tls_receive(s, limit);
    if (r->desc != &mim_desc_Ok) return r;
    mim_val *t = ((mim_val **)r->data)[0];
    mim_val *b = alloc(MIM_U8, 1, t->len);
    if (t->len) memcpy(b->data, t->data, (size_t)t->len);
    mim_release(r);
    return ok_of(b);
}

mim_val *mim_std_tls_close(mim_val *s) {
    int64_t id = geti(field_of(s, &mim_desc_tls_Stream, 0, "tls::close"), 0);
    pthread_mutex_lock(&tls_tab_lock);
    mconn *c = id >= 0 && id < tls_streams_n ? tls_streams[id] : NULL;
    if (c) tls_streams[id] = NULL;
    pthread_mutex_unlock(&tls_tab_lock);
    if (c) {
        conn_close(c);
        free(c);
    }
    return NULL;
}

/* ---- tls::self_signed: a certificate for development ------------------ */

static void db_wrap(dbuf *out, uint8_t tag, dbuf *in) {
    db_tlv(out, tag, in->p, in->n);
    free(in->p);
    in->p = NULL;
    in->n = in->cap = 0;
}

static void db_time(dbuf *b, time_t t) {
    struct tm g;
    gmtime_r(&t, &g);
    char buf[20];
    if (g.tm_year + 1900 < 2050) {
        strftime(buf, sizeof buf, "%y%m%d%H%M%SZ", &g);
        db_tlv(b, 0x17, (const uint8_t *)buf, strlen(buf)); /* UTCTime */
    } else {
        strftime(buf, sizeof buf, "%Y%m%d%H%M%SZ", &g);
        db_tlv(b, 0x18, (const uint8_t *)buf, strlen(buf)); /* GeneralizedTime */
    }
}

static void db_name(dbuf *b, const char *cn) {
    static const uint8_t oid_cn[] = {0x55, 0x04, 0x03};
    dbuf atv = {0}, set = {0}, seq = {0};
    db_tlv(&atv, 0x06, oid_cn, sizeof oid_cn);
    db_tlv(&atv, 0x0c, (const uint8_t *)cn, strlen(cn)); /* UTF8String */
    db_wrap(&set, 0x30, &atv);
    db_wrap(&seq, 0x31, &set);
    db_wrap(b, 0x30, &seq);
}

static void db_ext(dbuf *exts, const uint8_t *oid, size_t oidn, int critical, dbuf *value) {
    dbuf e = {0};
    db_tlv(&e, 0x06, oid, oidn);
    if (critical) db_put(&e, "\x01\x01\xff", 3);
    db_wrap(&e, 0x04, value);
    db_wrap(exts, 0x30, &e);
}

/* (certificate, key) as PEM: a fresh P-256 key and a certificate for `host`
 * signed by itself, valid for 90 days. Clients trust it only when told to
 * (tls::connect_trusting); browsers will warn. */
mim_val *mim_std_tls_self_signed(mim_val *hostv) {
    static const uint8_t oid_ecdsa_sha256[] = {0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02};
    static const uint8_t oid_ec_key[] = {0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01};
    static const uint8_t oid_p256[] = {0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07};
    static const uint8_t oid_san[] = {0x55, 0x1d, 0x11}, oid_basic[] = {0x55, 0x1d, 0x13}, oid_ku[] = {0x55, 0x1d, 0x0f},
                         oid_eku[] = {0x55, 0x1d, 0x25}, oid_server_auth[] = {0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01};
    const char *host = io_path(text_arg(hostv, "tls::self_signed"), "tls::self_signed");
    if (!*host) return err_of("tls::self_signed: the host name is empty");
    uint8_t pub[65], priv[32];
    char err[256];
    void *key = ec_keygen(pub, priv, err, sizeof err);
    if (!key) return err_of(err);

    dbuf tbs = {0}, t = {0}, u = {0};
    db_put(&tbs, "\xa0\x03\x02\x01\x02", 5); /* version 3 */
    uint8_t serial[16];
    arc4random_buf_or_urandom(serial, sizeof serial);
    serial[0] = (uint8_t)((serial[0] & 0x7f) | 0x01);
    db_tlv(&tbs, 0x02, serial, sizeof serial);
    db_tlv(&t, 0x06, oid_ecdsa_sha256, sizeof oid_ecdsa_sha256);
    db_wrap(&tbs, 0x30, &t);
    db_name(&tbs, host); /* issuer */
    time_t now = time(NULL);
    db_time(&t, now - 3600);
    db_time(&t, now + (time_t)90 * 86400);
    db_wrap(&tbs, 0x30, &t);
    db_name(&tbs, host); /* subject */
    db_tlv(&u, 0x06, oid_ec_key, sizeof oid_ec_key);
    db_tlv(&u, 0x06, oid_p256, sizeof oid_p256);
    db_wrap(&t, 0x30, &u);
    uint8_t bits[66];
    bits[0] = 0;
    memcpy(bits + 1, pub, 65);
    db_tlv(&t, 0x03, bits, sizeof bits);
    db_wrap(&tbs, 0x30, &t);

    /* extensions: the names it is for, not a CA, for TLS servers */
    dbuf exts = {0}, names = {0}, v = {0};
    unsigned char ip[16];
    if (inet_pton(AF_INET, host, ip) == 1) db_tlv(&names, 0x87, ip, 4);
    else if (inet_pton(AF_INET6, host, ip) == 1) db_tlv(&names, 0x87, ip, 16);
    else db_tlv(&names, 0x82, (const uint8_t *)host, strlen(host));
    if (strcmp(host, "localhost") == 0) {
        db_tlv(&names, 0x87, (const uint8_t *)"\x7f\x00\x00\x01", 4);
        db_tlv(&names, 0x87, (const uint8_t *)"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01", 16);
    }
    db_wrap(&v, 0x30, &names);
    db_ext(&exts, oid_san, sizeof oid_san, 0, &v);
    db_put(&v, "\x30\x00", 2);
    db_ext(&exts, oid_basic, sizeof oid_basic, 1, &v);
    db_put(&v, "\x03\x02\x07\x80", 4); /* digitalSignature */
    db_ext(&exts, oid_ku, sizeof oid_ku, 1, &v);
    db_tlv(&u, 0x06, oid_server_auth, sizeof oid_server_auth);
    db_wrap(&v, 0x30, &u);
    db_ext(&exts, oid_eku, sizeof oid_eku, 0, &v);
    db_wrap(&t, 0x30, &exts);
    db_wrap(&tbs, 0xa3, &t);
    dbuf tbs_seq = {0};
    db_wrap(&tbs_seq, 0x30, &tbs);

    dbuf sig = {0}, cert = {0}, out = {0};
    if (!ec_sign(key, tbs_seq.p, tbs_seq.n, &sig)) {
        ec_free(key);
        free(tbs_seq.p);
        return err_of("tls::self_signed: cannot sign the certificate");
    }
    ec_free(key);
    db_put(&cert, tbs_seq.p, tbs_seq.n);
    free(tbs_seq.p);
    db_tlv(&t, 0x06, oid_ecdsa_sha256, sizeof oid_ecdsa_sha256);
    db_wrap(&cert, 0x30, &t);
    db_put(&t, "\0", 1);
    db_put(&t, sig.p, sig.n);
    free(sig.p);
    db_wrap(&cert, 0x03, &t);
    db_wrap(&out, 0x30, &cert);
    size_t cn, kn;
    char *cert_pem = pem_wrap("CERTIFICATE", out.p, out.n, &cn);
    free(out.p);
    out.p = NULL;
    out.n = out.cap = 0;

    /* the key: SEC1 "EC PRIVATE KEY" with its curve and public key */
    dbuf k = {0};
    db_put(&k, "\x02\x01\x01", 3);
    db_tlv(&k, 0x04, priv, 32);
    db_tlv(&u, 0x06, oid_p256, sizeof oid_p256);
    db_wrap(&k, 0xa0, &u);
    db_tlv(&u, 0x03, bits, sizeof bits);
    db_wrap(&k, 0xa1, &u);
    db_wrap(&out, 0x30, &k);
    memset(priv, 0, sizeof priv);
    char *key_pem = pem_wrap("EC PRIVATE KEY", out.p, out.n, &kn);
    memset(out.p, 0, out.n);
    free(out.p);

    mim_val *pair[2] = {mim_str(cert_pem, (int64_t)cn), mim_str(key_pem, (int64_t)kn)};
    free(cert_pem);
    memset(key_pem, 0, kn);
    free(key_pem);
    return ok_of(obj_owned(&mim_desc_tuple, 2, pair));
}

static int64_t count_of(mim_val *n) { return geti(n, 0) > 0 ? geti(n, 0) : 1; }

mim_val *mim_std_http_serve_tls(mim_val *port, mim_val *handler, mim_val *identity) {
    return serve_loop_with(port, handler, 0, serve_keep, identity_of(identity, "http::serve_tls"));
}

mim_val *mim_std_http_serve_tls_requests(mim_val *port, mim_val *handler, mim_val *identity, mim_val *count) {
    return serve_loop_with(port, handler, count_of(count), serve_one, identity_of(identity, "http::serve_tls_requests"));
}

mim_val *mim_std_ws_serve_tls(mim_val *port, mim_val *handler, mim_val *identity) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve_tls: the handler must be a function");
    return serve_loop_with(port, handler, 0, ws_serve_one, identity_of(identity, "ws::serve_tls"));
}

mim_val *mim_std_ws_serve_tls_connections(mim_val *port, mim_val *handler, mim_val *identity, mim_val *count) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve_tls_connections: the handler must be a function");
    return serve_loop_with(port, handler, count_of(count), ws_serve_one, identity_of(identity, "ws::serve_tls_connections"));
}

/* ==== ui: a page and its WebSocket session on one port ================== */

/* does the request head ask for a WebSocket? */
static int wants_websocket(const char *head, size_t n) {
    for (size_t i = 0; i + 18 <= n; i++)
        if ((i == 0 || head[i - 1] == '\n') && ieq(head + i, "sec-websocket-key:", 18)) return 1;
    return 0;
}

/* `pair` holds (page handler, session handler) */
static void app_serve_one(mconn *c, mim_val *pair) {
    sock_timeout(c->fd, 30);
    sbuf b = {0};
    int64_t off = read_head(c, &b);
    if (off < 0) {
        free(b.p);
        conn_close(c);
        return;
    }
    mim_val **h = (mim_val **)pair->data;
    if (wants_websocket(b.p, (size_t)off)) ws_after_head(c, h[1], b, off);
    else {
        http_answer(c, h[0], &b, off, 0, NULL);
        free(b.p);
    }
}

/* serve HTTP requests with `page` and WebSocket connections with `session`
 * on one port; `count` > 0 stops after that many connections (page loads
 * and sessions; for tests), 0 serves forever */
mim_val *mim_std_ui_serve_app(mim_val *port, mim_val *page, mim_val *session, mim_val *count) {
    if (!page || page->dtype != MIM_FN || !session || session->dtype != MIM_FN) mim_panic("ui: the handlers must be functions");
    mim_retain(page);
    mim_retain(session);
    mim_val *two[2] = {page, session};
    mim_val *pair = obj_owned(&mim_desc_tuple, 2, two);
    /* tabs are independent (each has its own state): always side by side;
     * within a tab, messages are handled one at a time, in order */
    int parallel = MIM_THREADS;
    if (geti(count, 0) == 0) { /* serving for good: say where (a notice, not output) */
        printf("open http://localhost:%lld/\n", (long long)geti(port, 0));
        fflush(stdout);
    }
    mim_val *r = serve_loop_ex(port, pair, geti(count, 0), app_serve_one, NULL, parallel);
    mim_release(pair);
    return r;
}

/* the browser side of ui::run */
mim_val *mim_std_ui_client_script(void) { return mim_str(MIM_UI_JS, (int64_t)strlen(MIM_UI_JS)); }

#endif
