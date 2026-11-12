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
            co_wait_fd(fd, 1);
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

/* a TLS session over a connected socket, or NULL with a message */
static void *tls_open(int fd, const char *host, char *err, size_t errn) {
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
    OSStatus rc;
    while ((rc = st.handshake(ctx)) == -9841 /* errSSLPeerAuthCompleted */) {
    }
    if (rc != 0) {
        const char *why = st_error(rc);
        if (why) snprintf(err, errn, "TLS with %s failed: %s", host, why);
        else snprintf(err, errn, "TLS with %s failed (Security framework error %d)", host, (int)rc);
        st.release(ctx);
        return NULL;
    }
    return ctx;
}

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

static void *tls_open(int fd, const char *host, char *err, size_t errn) {
    if (!tls_load()) {
        snprintf(err, errn, "TLS is not available: install OpenSSL 3 (libssl.so.3); static builds cannot load it");
        return NULL;
    }
    void *ssl = os.ssl_new(os.ctx);
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
    if (t_co && os.pending && os.get_fd && os.pending(t) == 0) co_wait_fd(os.get_fd(t), 0);
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
static ssize_t tls_read(void *t, void *buf, size_t n) { (void)t; (void)buf; (void)n; return -1; }
static int tls_write_all(void *t, const char *p, size_t n) { (void)t; (void)p; (void)n; return -1; }
static void tls_close(void *t) { (void)t; }

#endif

/* ---- HTTP: shared parsing ----------------------------------------------- */

/* read until the end of the headers; returns the offset just past "\r\n\r\n"
 * (or -1), leaving any body bytes already read in `b` */
static int64_t read_head(mconn *c, sbuf *b) {
    char tmp[16384];
    for (;;) {
        for (size_t i = b->n >= 4 ? 0 : 0; i + 3 < b->n; i++)
            if (b->p[i] == '\r' && b->p[i + 1] == '\n' && b->p[i + 2] == '\r' && b->p[i + 3] == '\n') return (int64_t)i + 4;
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
    mconn conn = {fd, NULL};
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

/* one request on a connection: parse, call the handler, answer, close */
static void serve_one(int fd, mim_val *handler) {
    sock_timeout(fd, 30);
    mconn conn = {fd, NULL};
    sbuf b = {0};
    int64_t off = read_head(&conn, &b);
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
    mim_val *bodyv = read_body(&conn, &b, off, clen < 0 && !chunked ? 0 : clen, chunked, 0);
    mim_val *fields[5] = {
        mim_str(b.p, (int64_t)(sp1 - b.p)),
        mim_str(target, (int64_t)((q ? q : sp2) - target)),
        q ? mim_str(q + 1, (int64_t)(sp2 - q - 1)) : mim_str("", 0),
        hm,
        bodyv,
    };
    mim_val *req = obj_owned(&mim_desc_http_Request, 5, fields);
    free(b.p);
    mim_val *resp = mim_call(handler, 1, &req);
    mim_release(req);
    int64_t status = geti(field_of(resp, &mim_desc_http_Response, 0, "an HTTP handler's result"), 0);
    mim_val *rh = field_of(resp, &mim_desc_http_Response, 1, "an HTTP handler's result");
    mim_val *rb = field_of(resp, &mim_desc_http_Response, 2, "an HTTP handler's result");
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

typedef void (*conn_fn)(int fd, mim_val *handler);

typedef struct {
    int fd;
    mim_val *handler;
    conn_fn one;
} conn_job;

static void *conn_main(void *p) {
    conn_job *j = (conn_job *)p;
    j->one(j->fd, j->handler);
    mim_release(j->handler);
    free(j);
    free_list_drain(); /* this thread ends now */
    return NULL;
}

#if MIM_GREEN
static void conn_co_main(void *p) {
    conn_job *j = (conn_job *)p;
    j->one(j->fd, j->handler);
    mim_release(j->handler);
    free(j);
}
#endif

/* accept `limit` connections (0 = forever); handlers whose effects may
 * overlap get a task each (a coroutine on the event loop, else a thread),
 * others run one at a time in order */
static mim_val *serve_loop_with(mim_val *port, mim_val *handler, int64_t limit, conn_fn one);

static mim_val *serve_loop(mim_val *port, mim_val *handler, int64_t limit) { return serve_loop_with(port, handler, limit, serve_one); }

/* accept `limit` connections (0 = forever) and run `one` on each */
static mim_val *serve_loop_with(mim_val *port, mim_val *handler, int64_t limit, conn_fn one) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("serve: the handler must be a function");
    mim_val *lr = mim_std_tcp_listen(port);
    if (lr->desc != &mim_desc_Ok) return lr;
    mim_val *l = ((mim_val **)lr->data)[0];
    int lfd = (int)geti(((mim_val **)l->data)[0], 0);
    mim_release(lr);
    int parallel = MIM_THREADS && FN_DESC(handler)->pure;
    int64_t served = 0;
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
            one(fd, handler);
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
    close(lfd);
    return ok_of(mim_int(served));
}

mim_val *mim_std_http_serve(mim_val *port, mim_val *handler) { return serve_loop(port, handler, 0); }

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
    mconn c = {fd, NULL};
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
static void ws_serve_one(int fd, mim_val *handler) {
    sock_timeout(fd, 30);
    mconn c = {fd, NULL};
    sbuf b = {0};
    int64_t off = read_head(&c, &b);
    if (off < 0) {
        free(b.p);
        close(fd);
        return;
    }
    const char *first_end = strstr(b.p, "\r\n");
    int64_t clen;
    int chunked;
    mim_val *hm = parse_headers(first_end + 2, b.p + off - 2, &clen, &chunked);
    mim_val *k = mim_str("sec-websocket-key", 17);
    int64_t at = map_find(hm, k, NULL);
    mim_release(k);
    if (at < 0) {
        const char *bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 27\r\nConnection: close\r\n\r\nexpected a WebSocket client";
        send_all(fd, bad, strlen(bad));
        mim_release(hm);
        free(b.p);
        close(fd);
        return;
    }
    mim_val *key = ((mim_val **)hm->data)[2 * at + 1];
    char acc[32], resp[256];
    ws_accept_of((const char *)key->data, (size_t)key->len, acc);
    mim_release(hm);
    snprintf(resp, sizeof resp, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n", acc);
    if (send_all(fd, resp, strlen(resp)) != 0) {
        free(b.p);
        close(fd);
        return;
    }
    sock_timeout(fd, 0);
    ws_conn *w = ws_new(fd, NULL, 0, b.p + off, b.n - (size_t)off);
    free(b.p);
    mim_val *sock = ws_socket_of(w);
    mim_val *r = mim_call(handler, 1, &sock);
    if (r) mim_release(r);
    ws_drop(sock); /* closes it, if the handler did not */
    mim_release(sock);
}

mim_val *mim_std_ws_serve(mim_val *port, mim_val *handler) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve: the handler must be a function");
    return serve_loop_with(port, handler, 0, ws_serve_one);
}

mim_val *mim_std_ws_serve_connections(mim_val *port, mim_val *handler, mim_val *count) {
    if (!handler || handler->dtype != MIM_FN) mim_panic("ws::serve_connections: the handler must be a function");
    return serve_loop_with(port, handler, geti(count, 0) > 0 ? geti(count, 0) : 1, ws_serve_one);
}

#endif
