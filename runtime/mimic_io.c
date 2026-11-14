/* The standard modules with effects: fs, path, io, os, time, random, plus
 * implicit futures. Included at the end of mimic_rt.c (same translation unit).
 *
 * Each module function is exported as mim_std_<module>_<name> (see the end of
 * this file) and returns the standard library's own values (`Ok(…)`,
 * `Err("…")`, `Some(…)`, records), built from the type descriptors the
 * compiler exports by name (mim_desc_Ok, …). Their effects are declared in
 * the prelude (`extern "mimic" concurrent { … }`, see docs/ffi.md). */

#include <dirent.h>
#include <sys/stat.h>
#include <fcntl.h>
#ifndef _WIN32
#include <unistd.h>
#else
#include <direct.h> /* _mkdir, _getcwd, _chdir */
#include <io.h>
#endif


/* ---- standard-library values -------------------------------------------- */

extern const mim_desc mim_desc_Ok, mim_desc_Err, mim_desc_Some, mim_desc_None;
extern const mim_desc mim_desc_os_Output, mim_desc_time_DateTime;

static mim_val *obj_owned(const mim_desc *d, int64_t n, mim_val **fields) {
    mim_val *r = mim_obj(d, n, fields); /* retains the fields */
    for (int64_t i = 0; i < n; i++) mim_release(fields[i]);
    return r;
}

static mim_val *ok_of(mim_val *v) { return obj_owned(&mim_desc_Ok, 1, &v); }
static mim_val *some_of(mim_val *v) { return obj_owned(&mim_desc_Some, 1, &v); }
static mim_val *none_val(void) { return obj_owned(&mim_desc_None, 0, NULL); }

static mim_val *err_of(const char *msg) {
    mim_val *m = mim_from_cstr(msg);
    return obj_owned(&mim_desc_Err, 1, &m);
}

static _Thread_local char io_err[512];
static _Thread_local int io_failed;

static void io_clear(void) {
    io_failed = 0;
    io_err[0] = 0;
}

/* record `what: path: strerror(errno)` as the last error */
static void io_fail(const char *what, const char *path) {
    io_failed = 1;
    if (path) snprintf(io_err, sizeof io_err, "%s %s: %s", what, path, strerror(errno));
    else snprintf(io_err, sizeof io_err, "%s: %s", what, strerror(errno));
}

static void io_fail_msg(const char *msg) {
    io_failed = 1;
    snprintf(io_err, sizeof io_err, "%s", msg);
}

static const char *io_path(mim_val *v, const char *fn) {
    if (!v || v->dtype != MIM_CHAR || v->rank != 1) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected a path (str)", fn);
        mim_panic(buf);
    }
    return (const char *)v->data; /* strings are NUL-terminated */
}


/* ---- program ------------------------------------------------------------ */

/* the command-line arguments, without the program's own name */
static mim_val *impl_args(void) {
    int64_t n = g_argc > 1 ? g_argc - 1 : 0;
    if (n == 0) return alloc(MIM_BOXED, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    for (int64_t i = 0; i < n; i++) tmp[i] = mim_from_cstr(g_argv[i + 1]);
    mim_val *r = mim_array_lit(n, tmp);
    for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
    free(tmp);
    return r;
}

static mim_val *impl_io_env(mim_val *name) {
    io_clear();
    const char *v = getenv(io_path(name, "env"));
    if (!v) {
        io_fail_msg("not set");
        return mim_str("", 0);
    }
    return mim_from_cstr(v);
}

static mim_val *impl_io_set_env(mim_val *name, mim_val *value) {
    io_clear();
#ifdef _WIN32
    if (_putenv_s(io_path(name, "set_env"), io_path(value, "set_env")) != 0) io_fail("set_env", io_path(name, "set_env"));
#else
    if (setenv(io_path(name, "set_env"), io_path(value, "set_env"), 1) != 0) io_fail("set_env", io_path(name, "set_env"));
#endif
    return mim_bool(!io_failed);
}

/* print to standard error (a newline follows) */
static mim_val *impl_eprint(mim_val *v) {
    fflush(stdout);
    sbuf b = {0};
    fmt_val(&b, v, 0);
    if (b.n) fwrite(b.p, 1, b.n, stderr);
    fputc('\n', stderr);
    free(b.p);
    return NULL;
}

/* leave the program now with an exit status */
static mim_val *impl_exit(mim_val *code) {
    fflush(stdout);
    fflush(stderr);
    exit((int)(code && code->rank == 0 && !IS_FLOAT(code->dtype) ? geti(code, 0) : 1));
    return NULL;
}

/* ---- standard input ----------------------------------------------------- */

/* one line without its newline; at end of input: error "end of input" */
static mim_val *impl_io_read_line(void) {
    io_clear();
    fflush(stdout);
    sbuf b = {0};
    int c, any = 0;
    while ((c = getchar()) != EOF) {
        any = 1;
        if (c == '\n') break;
        char ch = (char)c;
        sb_put(&b, &ch, 1);
    }
    if (!any) {
        free(b.p);
        io_fail_msg("end of input");
        return mim_str("", 0);
    }
    if (b.n && b.p[b.n - 1] == '\r') b.n--;
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

/* everything left on standard input */
static mim_val *impl_read_input(void) {
    sbuf b = {0};
    char buf[65536];
    size_t n;
    while ((n = fread(buf, 1, sizeof buf, stdin)) > 0) sb_put(&b, buf, n);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

/* ---- files -------------------------------------------------------------- */

static mim_val *impl_io_read_file(mim_val *path) {
    io_clear();
    const char *p = io_path(path, "read_file");
    FILE *f = fopen(p, "rb");
    if (!f) {
        io_fail("cannot read", p);
        return mim_str("", 0);
    }
    sbuf b = {0};
    char buf[65536];
    size_t n;
    while ((n = fread(buf, 1, sizeof buf, f)) > 0) sb_put(&b, buf, n);
    int bad = ferror(f);
    fclose(f);
    if (bad) {
        free(b.p);
        io_fail("cannot read", p);
        return mim_str("", 0);
    }
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

/* write (or append) text; the number of bytes written */
static mim_val *impl_io_write_file(mim_val *path, mim_val *text, mim_val *append) {
    io_clear();
    const char *p = io_path(path, "write_file");
    if (!text || text->dtype != MIM_CHAR || text->rank != 1) mim_panic("write_file: expected text (str)");
    FILE *f = fopen(p, append && geti(append, 0) ? "ab" : "wb");
    if (!f) {
        io_fail("cannot write", p);
        return mim_int(0);
    }
    size_t n = fwrite(text->data, 1, (size_t)text->len, f);
    int bad = n != (size_t)text->len;
    if (fclose(f) != 0) bad = 1;
    if (bad) io_fail("cannot write", p);
    return mim_int((int64_t)n);
}

static int stat_of(const char *p, struct stat *st) { return stat(p, st) == 0; }

static mim_val *impl_exists(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "exists"), &st));
}

static mim_val *impl_is_dir(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "is_dir"), &st) && S_ISDIR(st.st_mode));
}

static mim_val *impl_is_file(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "is_file"), &st) && S_ISREG(st.st_mode));
}

static mim_val *impl_io_file_size(mim_val *path) {
    io_clear();
    struct stat st;
    const char *p = io_path(path, "file_size");
    if (!stat_of(p, &st)) {
        io_fail("cannot stat", p);
        return mim_int(0);
    }
    return mim_int((int64_t)st.st_size);
}

/* remove a file or an empty directory */
static mim_val *impl_io_remove_file(mim_val *path) {
    io_clear();
    const char *p = io_path(path, "remove");
    struct stat st;
    int ok = stat_of(p, &st) && (S_ISDIR(st.st_mode) ? rmdir(p) == 0 : unlink(p) == 0);
    if (!ok) io_fail("cannot remove", p);
    return mim_bool(ok);
}

static mim_val *impl_io_rename(mim_val *from, mim_val *to) {
    io_clear();
    const char *a = io_path(from, "rename"), *b = io_path(to, "rename");
    if (rename(a, b) != 0) io_fail("cannot rename", a);
    return mim_bool(!io_failed);
}

static mim_val *impl_io_copy_file(mim_val *from, mim_val *to) {
    io_clear();
    const char *a = io_path(from, "copy_file"), *b = io_path(to, "copy_file");
    FILE *in = fopen(a, "rb");
    if (!in) {
        io_fail("cannot read", a);
        return mim_int(0);
    }
    FILE *out = fopen(b, "wb");
    if (!out) {
        fclose(in);
        io_fail("cannot write", b);
        return mim_int(0);
    }
    char buf[65536];
    size_t n;
    int64_t total = 0;
    while ((n = fread(buf, 1, sizeof buf, in)) > 0) {
        if (fwrite(buf, 1, n, out) != n) {
            io_fail("cannot write", b);
            break;
        }
        total += (int64_t)n;
    }
    fclose(in);
    if (fclose(out) != 0 && !io_failed) io_fail("cannot write", b);
    return mim_int(total);
}

/* ---- directories -------------------------------------------------------- */

static int cmp_cstr(const void *x, const void *y) { return strcmp(*(char *const *)x, *(char *const *)y); }

/* the names in a directory (no "." or ".."), sorted */
static mim_val *impl_io_list_dir(mim_val *path) {
    io_clear();
    const char *p = io_path(path, "list_dir");
    DIR *d = opendir(p);
    if (!d) {
        io_fail("cannot list", p);
        return alloc(MIM_BOXED, 1, 0);
    }
    char **names = NULL;
    int64_t n = 0, cap = 0;
    struct dirent *e;
    while ((e = readdir(d))) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        if (n == cap) {
            cap = cap ? cap * 2 : 16;
            names = (char **)realloc(names, sizeof(char *) * (size_t)cap);
        }
        names[n++] = strdup(e->d_name);
    }
    closedir(d);
    qsort(names, (size_t)n, sizeof(char *), cmp_cstr);
    mim_val *r;
    if (n == 0) {
        r = alloc(MIM_BOXED, 1, 0);
    } else {
        mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
        for (int64_t i = 0; i < n; i++) tmp[i] = mim_from_cstr(names[i]);
        r = mim_array_lit(n, tmp);
        for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
        free(tmp);
    }
    for (int64_t i = 0; i < n; i++) free(names[i]);
    free(names);
    return r;
}

static int mkdir_one(const char *p) {
#ifdef _WIN32
    return _mkdir(p);
#else
    return mkdir(p, 0777);
#endif
}

/* create a directory and any missing parents; fine if it already exists */
static mim_val *impl_io_make_dir(mim_val *path) {
    io_clear();
    const char *p = io_path(path, "make_dir");
    size_t len = strlen(p);
    char *buf = (char *)malloc(len + 1);
    memcpy(buf, p, len + 1);
    for (size_t i = 1; i <= len; i++) {
        if (buf[i] == '/' || buf[i] == 0) {
            char saved = buf[i];
            buf[i] = 0;
            struct stat st;
            if (!stat_of(buf, &st)) {
                if (mkdir_one(buf) != 0 && errno != EEXIST) {
                    io_fail("cannot create", buf);
                    free(buf);
                    return mim_bool(0);
                }
            } else if (!S_ISDIR(st.st_mode)) {
                errno = ENOTDIR;
                io_fail("cannot create", buf);
                free(buf);
                return mim_bool(0);
            }
            buf[i] = saved;
        }
    }
    free(buf);
    return mim_bool(1);
}

static mim_val *impl_current_dir(void) {
    char buf[4096];
    if (!getcwd(buf, sizeof buf)) return mim_str(".", 1);
    return mim_from_cstr(buf);
}

static mim_val *impl_temp_dir(void) {
    const char *t = getenv("TMPDIR");
    if (!t || !*t) t = "/tmp";
    size_t n = strlen(t);
    while (n > 1 && t[n - 1] == '/') n--; /* no trailing slash */
    return mim_str(t, (int64_t)n);
}

/* ---- paths (pure) ------------------------------------------------------- */

/* "a/b" + "c" → "a/b/c"; an absolute second part replaces the first */
static mim_val *impl_path_join(mim_val *a, mim_val *b) {
    const char *x = io_path(a, "path_join"), *y = io_path(b, "path_join");
    if (y[0] == '/' || x[0] == 0) return mim_from_cstr(y);
    size_t nx = strlen(x), ny = strlen(y);
    int slash = x[nx - 1] != '/';
    char *buf = (char *)malloc(nx + ny + 2);
    memcpy(buf, x, nx);
    if (slash) buf[nx] = '/';
    memcpy(buf + nx + slash, y, ny);
    mim_val *r = mim_str(buf, (int64_t)(nx + slash + ny));
    free(buf);
    return r;
}

/* the last component: "a/b/c.txt" → "c.txt" */
static mim_val *impl_file_name(mim_val *path) {
    const char *p = io_path(path, "file_name");
    size_t n = strlen(p);
    while (n > 1 && p[n - 1] == '/') n--;
    size_t s = n;
    while (s > 0 && p[s - 1] != '/') s--;
    return mim_str(p + s, (int64_t)(n - s));
}

/* everything before the last component: "a/b/c.txt" → "a/b"; "c" → "" */
static mim_val *impl_parent(mim_val *path) {
    const char *p = io_path(path, "parent");
    size_t n = strlen(p);
    while (n > 1 && p[n - 1] == '/') n--;
    while (n > 0 && p[n - 1] != '/') n--;
    while (n > 1 && p[n - 1] == '/') n--;
    return mim_str(p, (int64_t)n);
}

/* "c.tar.gz" → "gz"; no dot (or a leading dot only) → "" */
static mim_val *impl_extension(mim_val *path) {
    mim_val *f = impl_file_name(path);
    const char *s = (const char *)f->data;
    const char *dot = strrchr(s, '.');
    mim_val *r = dot && dot != s ? mim_from_cstr(dot + 1) : mim_str("", 0);
    mim_release(f);
    return r;
}

/* the file name without its extension: "a/report.pdf" → "report" */
static mim_val *impl_stem(mim_val *path) {
    mim_val *f = impl_file_name(path);
    const char *s = (const char *)f->data;
    const char *dot = strrchr(s, '.');
    mim_val *r = dot && dot != s ? mim_str(s, (int64_t)(dot - s)) : (mim_retain(f), f);
    mim_release(f);
    return r;
}

/* ==== E2: time, random numbers, processes ================================= */

#include <time.h>
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif
/* platforms without child processes: WASI, and Apple's tvOS / watchOS */
#if defined(__wasi__) || (defined(__APPLE__) && (TARGET_OS_TV || TARGET_OS_WATCH))
#define MIM_NO_SPAWN 1
#endif
#if !defined(MIM_NO_SPAWN) && !defined(_WIN32)
#include <spawn.h>
#include <poll.h>
#include <sys/wait.h>
extern char **environ;
#endif

/* ---- clocks ------------------------------------------------------------- */

/* wall-clock time: nanoseconds since 1970-01-01 00:00:00 UTC */
static mim_val *impl_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return mim_int((int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec);
}

/* a monotonic clock in nanoseconds: for measuring how long things take */
static mim_val *impl_clock(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return mim_int((int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec);
}

static mim_val *impl_sleep(mim_val *d) {
    int64_t ns = d && d->rank == 0 ? (IS_FLOAT(d->dtype) ? (int64_t)getf(d, 0) : geti(d, 0)) : 0;
    if (ns <= 0) return NULL;
#if MIM_GREEN
    if (t_co) { /* a coroutine waits on the event loop's timer */
        co_sleep(ns);
        return NULL;
    }
#endif
    struct timespec ts = {(time_t)(ns / 1000000000), (long)(ns % 1000000000)};
    while (nanosleep(&ts, &ts) != 0 && errno == EINTR) {
    }
    return NULL;
}

/* days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant) */
static int64_t days_from_civil(int64_t y, int64_t m, int64_t d) {
    y -= m <= 2;
    int64_t era = (y >= 0 ? y : y - 399) / 400;
    int64_t yoe = y - era * 400;
    int64_t doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + doe - 719468;
}

static void civil_from_days(int64_t z, int64_t *y, int64_t *m, int64_t *d) {
    z += 719468;
    int64_t era = (z >= 0 ? z : z - 146096) / 146097;
    int64_t doe = z - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    int64_t mp = (5 * doy + 2) / 153;
    *d = doy - (153 * mp + 2) / 5 + 1;
    *m = mp + (mp < 10 ? 3 : -9);
    *y = yoe + era * 400 + (*m <= 2);
}

static int64_t floordiv64(int64_t a, int64_t b) { return a / b - ((a % b != 0) && ((a < 0) != (b < 0))); }

/* [year month day hour minute second nanosecond weekday(1=Mon) utc_offset(s)] */
static mim_val *date_parts(int64_t t, int64_t offset) {
    int64_t local = t + offset * 1000000000;
    int64_t secs = floordiv64(local, 1000000000), ns = local - secs * 1000000000;
    int64_t days = floordiv64(secs, 86400), sod = secs - days * 86400;
    int64_t y, m, d;
    civil_from_days(days, &y, &m, &d);
    int64_t wd = ((days % 7) + 7 + 3) % 7 + 1; /* 1970-01-01 was a Thursday */
    mim_val *r = alloc(MIM_I64, 1, 9);
    int64_t *p = (int64_t *)r->data;
    p[0] = y; p[1] = m; p[2] = d; p[3] = sod / 3600; p[4] = sod / 60 % 60; p[5] = sod % 60;
    p[6] = ns; p[7] = wd; p[8] = offset;
    return r;
}

static mim_val *impl_time_utc_parts(mim_val *t) { return date_parts(geti(t, 0), 0); }

/* the local time zone's offset from UTC at instant t, in seconds */
static int64_t local_offset(int64_t t) {
    time_t s = (time_t)floordiv64(t, 1000000000);
    struct tm lt;
#ifdef _WIN32
    localtime_s(&lt, &s);
    struct tm gt;
    gmtime_s(&gt, &s);
#else
    localtime_r(&s, &lt);
    struct tm gt;
    gmtime_r(&s, &gt);
#endif
    int64_t l = days_from_civil(lt.tm_year + 1900, lt.tm_mon + 1, lt.tm_mday) * 86400 + lt.tm_hour * 3600 + lt.tm_min * 60 + lt.tm_sec;
    int64_t g = days_from_civil(gt.tm_year + 1900, gt.tm_mon + 1, gt.tm_mday) * 86400 + gt.tm_hour * 3600 + gt.tm_min * 60 + gt.tm_sec;
    return l - g;
}

static mim_val *impl_time_local_parts(mim_val *t) {
    int64_t v = geti(t, 0);
    return date_parts(v, local_offset(v));
}

/* the instant of a UTC calendar date and time (ns since the epoch) */
static mim_val *impl_timestamp(mim_val *y, mim_val *mo, mim_val *d, mim_val *h, mim_val *mi, mim_val *s) {
    int64_t days = days_from_civil(geti(y, 0), geti(mo, 0), geti(d, 0));
    int64_t secs = days * 86400 + geti(h, 0) * 3600 + geti(mi, 0) * 60 + geti(s, 0);
    return mim_int(secs * 1000000000);
}

/* ISO 8601 / RFC 3339 in UTC: 2026-11-12T02:40:05Z (milliseconds when present) */
static mim_val *impl_format_time(mim_val *t) {
    mim_val *p = date_parts(geti(t, 0), 0);
    int64_t *v = (int64_t *)p->data;
    char buf[64];
    int64_t ms = v[6] / 1000000;
    if (ms)
        snprintf(buf, sizeof buf, "%04lld-%02lld-%02lldT%02lld:%02lld:%02lld.%03lldZ", (long long)v[0], (long long)v[1], (long long)v[2],
                 (long long)v[3], (long long)v[4], (long long)v[5], (long long)ms);
    else
        snprintf(buf, sizeof buf, "%04lld-%02lld-%02lldT%02lld:%02lld:%02lldZ", (long long)v[0], (long long)v[1], (long long)v[2],
                 (long long)v[3], (long long)v[4], (long long)v[5]);
    mim_release(p);
    return mim_from_cstr(buf);
}

/* ---- random numbers: xoshiro256**, one generator per thread ------------- */

static _Thread_local uint64_t rng_s[4];
static _Thread_local int rng_ready;

static uint64_t rotl64(uint64_t x, int k) { return (x << k) | (x >> (64 - k)); }

static uint64_t splitmix(uint64_t *x) {
    uint64_t z = (*x += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static void rng_seed(uint64_t seed) {
    for (int i = 0; i < 4; i++) rng_s[i] = splitmix(&seed);
    rng_ready = 1;
}

/* random bytes from the operating system; 0 when it gives none */
__attribute__((unused)) static int os_random(void *p, size_t n) {
#if defined(_WIN32)
    return BCryptGenRandom(NULL, (PUCHAR)p, (ULONG)n, BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0;
#elif defined(__APPLE__)
    arc4random_buf(p, n);
    return 1;
#else
    size_t got = 0;
    while (got < n) {
        size_t k = n - got > 256 ? 256 : n - got;
        if (getentropy((char *)p + got, k) != 0) return 0;
        got += k;
    }
    return 1;
#endif
}

static uint64_t rng_next(void) {
    if (!rng_ready) {
        uint64_t seed = 0;
#if defined(_WIN32)
        if (!os_random(&seed, sizeof seed)) seed = (uint64_t)time(NULL) ^ (uint64_t)(uintptr_t)&seed;
#elif defined(__APPLE__)
        arc4random_buf(&seed, sizeof seed); /* every Apple platform's libc */
#else
        if (getentropy(&seed, sizeof seed) != 0) {
            struct timespec ts;
            clock_gettime(CLOCK_REALTIME, &ts);
            seed = (uint64_t)ts.tv_nsec ^ ((uint64_t)ts.tv_sec << 30) ^ (uint64_t)(uintptr_t)&seed;
        }
#endif
        rng_seed(seed);
    }
    uint64_t r = rotl64(rng_s[1] * 5, 7) * 9, t = rng_s[1] << 17;
    rng_s[2] ^= rng_s[0];
    rng_s[3] ^= rng_s[1];
    rng_s[1] ^= rng_s[2];
    rng_s[0] ^= rng_s[3];
    rng_s[2] ^= t;
    rng_s[3] = rotl64(rng_s[3], 45);
    return r;
}

/* a float in [0, 1) */
static mim_val *impl_random(void) { return mim_float((double)(rng_next() >> 11) * 0x1.0p-53); }

/* an integer in [lo, hi), without modulo bias */
static mim_val *impl_random_int(mim_val *lo, mim_val *hi) {
    int64_t a = geti(lo, 0), b = geti(hi, 0);
    if (b <= a) mim_panic("random_int(lo, hi) needs lo < hi");
    uint64_t span = (uint64_t)b - (uint64_t)a, limit = UINT64_MAX - UINT64_MAX % span, x;
    do x = rng_next(); while (x >= limit);
    return mim_int((int64_t)((uint64_t)a + x % span));
}

/* make this thread's random numbers repeatable */
static mim_val *impl_seed_random(mim_val *seed) {
    rng_seed((uint64_t)geti(seed, 0));
    return NULL;
}

/* ---- processes ---------------------------------------------------------- */

static mim_val *impl_pid(void) {
#if defined(_WIN32) || defined(__wasi__)
    return mim_int(0);
#else
    return mim_int((int64_t)getpid());
#endif
}

#ifdef _WIN32
/* UTF-8 → a new UTF-16 string (free it) */
static wchar_t *win_wide(const char *s) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, NULL, 0);
    wchar_t *w = (wchar_t *)malloc(sizeof(wchar_t) * (size_t)(n > 0 ? n : 1));
    if (n > 0) MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n);
    else w[0] = 0;
    return w;
}

/* one argument quoted the way Windows programs split their command line
 * (CommandLineToArgvW / the C runtime): quotes around anything with spaces
 * or quotes, backslashes doubled only before a quote */
static void win_quote(sbuf *b, const char *a) {
    if (*a && !strpbrk(a, " \t\n\v\"")) {
        sb_put(b, a, strlen(a));
        return;
    }
    sb_put(b, "\"", 1);
    for (const char *p = a;; p++) {
        size_t bs = 0;
        while (*p == '\\') {
            p++;
            bs++;
        }
        if (!*p) {
            for (size_t i = 0; i < bs * 2; i++) sb_put(b, "\\", 1);
            break;
        }
        size_t k = *p == '"' ? bs * 2 + 1 : bs;
        for (size_t i = 0; i < k; i++) sb_put(b, "\\", 1);
        sb_put(b, p, 1);
    }
    sb_put(b, "\"", 1);
}

typedef struct { HANDLE h; sbuf out; } win_reader;

static void *win_read_all(void *arg) {
    win_reader *r = (win_reader *)arg;
    char buf[65536];
    DWORD got;
    while (ReadFile(r->h, buf, sizeof buf, &got, NULL) && got > 0) sb_put(&r->out, buf, got);
    return NULL;
}

/* Run a command line (UTF-8); stdin is empty, stdout and stderr captured.
 * [status stdout stderr] */
static mim_val *win_run(const char *what, const char *cmdline) {
    SECURITY_ATTRIBUTES sa = {sizeof sa, NULL, TRUE};
    HANDLE out_r, out_w, err_r, err_w;
    if (!CreatePipe(&out_r, &out_w, &sa, 0) || !CreatePipe(&err_r, &err_w, &sa, 0)) {
        io_fail_msg("cannot run: no pipes");
        return alloc(MIM_BOXED, 1, 0);
    }
    SetHandleInformation(out_r, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(err_r, HANDLE_FLAG_INHERIT, 0);
    HANDLE nul = CreateFileW(L"NUL", GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, &sa, OPEN_EXISTING, 0, NULL);
    STARTUPINFOW si;
    memset(&si, 0, sizeof si);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = nul;
    si.hStdOutput = out_w;
    si.hStdError = err_w;
    PROCESS_INFORMATION pi;
    wchar_t *wcmd = win_wide(cmdline);
    fflush(stdout);
    fflush(stderr);
    BOOL ok = CreateProcessW(NULL, wcmd, NULL, NULL, TRUE, CREATE_NO_WINDOW, NULL, NULL, &si, &pi);
    DWORD launch_err = GetLastError();
    free(wcmd);
    CloseHandle(out_w);
    CloseHandle(err_w);
    if (nul != INVALID_HANDLE_VALUE) CloseHandle(nul);
    if (!ok) {
        CloseHandle(out_r);
        CloseHandle(err_r);
        char msg[512];
        char sys[400] = "";
        FormatMessageA(FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS, NULL, launch_err, MAKELANGID(LANG_ENGLISH, SUBLANG_ENGLISH_US), sys, sizeof sys, NULL);
        size_t L = strlen(sys);
        while (L && (sys[L - 1] == '\n' || sys[L - 1] == '\r' || sys[L - 1] == '.')) sys[--L] = 0;
        snprintf(msg, sizeof msg, "cannot run %s: %s", what, sys);
        io_fail_msg(msg);
        return alloc(MIM_BOXED, 1, 0);
    }
    /* read both pipes together so a full one never blocks the child */
    win_reader re = {err_r, {0}};
    pthread_t th;
    int threaded = pthread_create(&th, NULL, win_read_all, &re) == 0;
    win_reader ro = {out_r, {0}};
    win_read_all(&ro);
    if (threaded) pthread_join(th, NULL);
    else win_read_all(&re);
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD code = 1;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    CloseHandle(out_r);
    CloseHandle(err_r);
    mim_val *parts[3] = {mim_int((int64_t)code), mim_str(ro.out.p ? ro.out.p : "", (int64_t)ro.out.n), mim_str(re.out.p ? re.out.p : "", (int64_t)re.out.n)};
    free(ro.out.p);
    free(re.out.p);
    mim_val *r = mim_array_lit(3, parts);
    for (int k = 0; k < 3; k++) mim_release(parts[k]);
    return r;
}
#endif

/* run a program (looked up in PATH) with arguments; stdin is empty.
 * [status stdout stderr]; a signal gives status 128 + signal number */
static mim_val *impl_io_run(mim_val *program, mim_val *argv_list) {
    io_clear();
#if defined(_WIN32)
    const char *prog = io_path(program, "run");
    need(argv_list, "run");
    int64_t n = argv_list->rank == 1 ? argv_list->len : 0;
    if (n && argv_list->dtype != MIM_BOXED) mim_panic("run: the arguments must be a list of str");
    sbuf cmd = {0};
    win_quote(&cmd, prog);
    for (int64_t i = 0; i < n; i++) {
        sb_put(&cmd, " ", 1);
        win_quote(&cmd, io_path(((mim_val **)argv_list->data)[i], "run"));
    }
    sb_put(&cmd, "", 1);
    mim_val *r = win_run(prog, cmd.p);
    free(cmd.p);
    return r;
#elif defined(MIM_NO_SPAWN)
    (void)program;
    (void)argv_list;
    io_fail_msg("running programs is not supported on this platform");
    return alloc(MIM_BOXED, 1, 0);
#else
    const char *prog = io_path(program, "run");
    need(argv_list, "run");
    int64_t n = argv_list->rank == 1 ? argv_list->len : 0;
    if (n && argv_list->dtype != MIM_BOXED) mim_panic("run: the arguments must be a list of str");
    char **argv = (char **)calloc((size_t)n + 2, sizeof(char *));
    argv[0] = (char *)prog;
    for (int64_t i = 0; i < n; i++) argv[i + 1] = (char *)io_path(((mim_val **)argv_list->data)[i], "run");
    int out[2], err[2];
    if (pipe(out) != 0 || pipe(err) != 0) {
        free(argv);
        io_fail("cannot run", prog);
        return alloc(MIM_BOXED, 1, 0);
    }
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0);
    posix_spawn_file_actions_adddup2(&fa, out[1], 1);
    posix_spawn_file_actions_adddup2(&fa, err[1], 2);
    posix_spawn_file_actions_addclose(&fa, out[0]);
    posix_spawn_file_actions_addclose(&fa, err[0]);
    fflush(stdout);
    fflush(stderr);
    pid_t pid;
    int rc = posix_spawnp(&pid, prog, &fa, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&fa);
    free(argv);
    close(out[1]);
    close(err[1]);
    if (rc != 0) {
        close(out[0]);
        close(err[0]);
        errno = rc;
        io_fail("cannot run", prog);
        return alloc(MIM_BOXED, 1, 0);
    }
    /* read both pipes together so a full one never blocks the child */
    sbuf so = {0}, se = {0};
    struct pollfd fds[2] = {{out[0], POLLIN, 0}, {err[0], POLLIN, 0}};
    int open = 2;
    char buf[65536];
    while (open > 0) {
        if (poll(fds, 2, -1) < 0) {
            if (errno == EINTR) continue;
            break;
        }
        for (int k = 0; k < 2; k++) {
            if (fds[k].fd < 0 || !(fds[k].revents & (POLLIN | POLLHUP | POLLERR))) continue;
            ssize_t r = read(fds[k].fd, buf, sizeof buf);
            if (r > 0) sb_put(k == 0 ? &so : &se, buf, (size_t)r);
            else if (r == 0 || errno != EINTR) {
                close(fds[k].fd);
                fds[k].fd = -1;
                open--;
            }
        }
    }
    int st = 0;
    while (waitpid(pid, &st, 0) < 0 && errno == EINTR) {
    }
    int64_t status = WIFEXITED(st) ? WEXITSTATUS(st) : WIFSIGNALED(st) ? 128 + WTERMSIG(st) : 1;
    mim_val *parts[3] = {mim_int(status), mim_str(so.p ? so.p : "", (int64_t)so.n), mim_str(se.p ? se.p : "", (int64_t)se.n)};
    free(so.p);
    free(se.p);
    mim_val *r = mim_array_lit(3, parts);
    for (int k = 0; k < 3; k++) mim_release(parts[k]);
    return r;
#endif
}

/* ==== E3: implicit futures ==================================================
 * `let x = f(a, b)` where f's only effects may overlap (reads, sleeping) is
 * started here on its own thread; the compiler awaits it right before the
 * first statement that uses x, before any effect that must stay in order,
 * and at the end of the block. Arguments are evaluated (in order) before. */

typedef struct mim_future {
    mim_val *f;
    int32_t n;
    mim_val **args;
    mim_val *result;
#if MIM_THREADS
    pthread_t th;
#endif
#if MIM_GREEN
    mim_co *co; /* started from a coroutine: a coroutine too */
#endif
    int started;
    int done;                /* without threads: has the deferred call run? */
    struct mim_future *next; /* without threads: the queue of deferred calls */
} mim_future;

/* Without threads (WASI) a future is deferred: it runs when awaited, or
 * earlier when a channel receive would otherwise wait for it. Overlapping
 * effects may move like that; anything that changes state awaits first. */
static mim_future *lazy_head, *lazy_tail;

static void future_main_now(mim_future *fu);

static void lazy_unlink(mim_future *fu) {
    mim_future **p = &lazy_head;
    mim_future *prev = NULL;
    while (*p && *p != fu) {
        prev = *p;
        p = &(*p)->next;
    }
    if (*p) {
        *p = fu->next;
        if (lazy_tail == fu) lazy_tail = prev;
    }
    fu->next = NULL;
}

/* run the oldest deferred call; false when there is none */
static int lazy_run_one(void) {
    mim_future *fu = lazy_head;
    if (!fu) return 0;
    lazy_unlink(fu);
    future_main_now(fu);
    return 1;
}

static void *future_main(void *p) {
    mim_future *fu = (mim_future *)p;
    fu->result = mim_call(fu->f, fu->n, fu->args);
    fu->done = 1;
    if (fu->started) free_list_drain(); /* this thread ends now */
    return NULL;
}

#if MIM_GREEN
static void future_co_main(void *p) {
    mim_future *fu = (mim_future *)p;
    fu->result = mim_call(fu->f, fu->n, fu->args);
    fu->done = 1;
}
#endif

static void future_main_now(mim_future *fu) {
    if (!fu->done) future_main(fu);
}

/* start f(args…); retains f and the arguments */
void *mim_spawn(mim_val *f, int32_t n, mim_val **args) {
    mim_future *fu = (mim_future *)calloc(1, sizeof(mim_future));
    if (!fu) mim_panic("out of memory");
    fu->f = f;
    mim_retain(f);
    fu->n = n;
    fu->args = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n ? n : 1));
    for (int32_t i = 0; i < n; i++) {
        fu->args[i] = args[i];
        mim_retain(args[i]);
    }
#if MIM_GREEN
    if (t_co) { /* tasks started by tasks on the event loop stay on it */
        fu->started = 1;
        fu->co = co_spawn(future_co_main, fu, 0);
        return fu;
    }
#endif
#if MIM_THREADS
    pthread_attr_t a;
    pthread_attr_init(&a);
    pthread_attr_setstacksize(&a, (size_t)32 << 20);
    fu->started = 1; /* read by the thread: set before it starts */
    if (pthread_create(&fu->th, &a, future_main, fu) != 0) fu->started = 0;
    pthread_attr_destroy(&a);
#endif
    if (!fu->started) { /* no threads (WASI) or none left: defer it */
        if (lazy_tail) lazy_tail->next = fu;
        else lazy_head = fu;
        lazy_tail = fu;
    }
    return fu;
}

/* the result of a started call (owned); frees the future */
mim_val *mim_await(void *h) {
    mim_future *fu = (mim_future *)h;
#if MIM_GREEN
    if (fu->co) co_join(fu->co);
    else
#endif
#if MIM_THREADS
    if (fu->started) pthread_join(fu->th, NULL);
#endif
    if (!fu->started && !fu->done) {
        lazy_unlink(fu);
        future_main_now(fu);
    }
    mim_val *r = fu->result;
    mim_release(fu->f);
    for (int32_t i = 0; i < fu->n; i++) mim_release(fu->args[i]);
    free(fu->args);
    free(fu);
    return r;
}

/* ---- ui: work that runs on its own ---------------------------------------
 * ui::detach(f) starts f() as a task nobody waits for (a coroutine on the
 * event loop, else a thread; without threads it runs now). The ui session
 * uses it for jobs and subscriptions, which report back through a channel. */

static void detach_run(void *p) {
    mim_val *f = (mim_val *)p;
    mim_val *r = mim_call(f, 0, NULL);
    if (r) mim_release(r);
    mim_release(f);
    __atomic_fetch_sub(&g_detached_running, 1, __ATOMIC_ACQ_REL);
}

#if MIM_THREADS
static void *detach_thread(void *p) {
    detach_run(p);
    free_list_drain(); /* this thread ends now */
    return NULL;
}
#endif

mim_val *mim_std_ui_detach(mim_val *f) {
    if (!f || f->dtype != MIM_FN) mim_panic("ui::detach: expected a function");
    mim_retain(f);
    __atomic_fetch_add(&g_detached_running, 1, __ATOMIC_ACQ_REL);
#if MIM_GREEN
    if (green_on()) {
        co_spawn(detach_run, f, 1);
        return NULL;
    }
#endif
#if MIM_THREADS
    pthread_t t;
    pthread_attr_t a;
    pthread_attr_init(&a);
    pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
    pthread_attr_setstacksize(&a, (size_t)32 << 20);
    int ok = pthread_create(&t, &a, detach_thread, f) == 0;
    pthread_attr_destroy(&a);
    if (ok) return NULL;
#endif
    detach_run(f);
    return NULL;
}

/* ui::then(state, job): the next state, and one job to run (ui::Step). Made
 * here rather than in Mimic: a job may have effects (that is its point), and
 * Mimic code cannot keep such a function in a value (E0605). */
extern const mim_desc mim_desc_ui_Step;

mim_val *mim_std_ui_then(mim_val *state, mim_val *job) {
    if (!job || job->dtype != MIM_FN) mim_panic("ui::then: expected a function");
    mim_retain(state);
    mim_val *f[2] = {state, mim_array_lit(1, &job)};
    return obj_owned(&mim_desc_ui_Step, 2, f);
}

/* ui::also(step, job): the same step with one more job */
mim_val *mim_std_ui_also(mim_val *step, mim_val *job) {
    if (!step || step->dtype != MIM_OBJ || step->desc != &mim_desc_ui_Step) mim_panic("ui::also: expected a ui::Step");
    if (!job || job->dtype != MIM_FN) mim_panic("ui::also: expected a function");
    mim_val **h = (mim_val **)step->data;
    mim_retain(h[0]);
    mim_val *f[2] = {h[0], mim_std_push(h[1], job)};
    return obj_owned(&mim_desc_ui_Step, 2, f);
}

/* ==== the module functions: mim_std_<module>_<name> ======================== */

/* the last error (if any) of an impl_ call as a Result around v */
static mim_val *io_result(mim_val *v) {
    if (io_failed) {
        mim_release(v);
        return err_of(io_err);
    }
    return ok_of(v);
}

/* ---- fs ----------------------------------------------------------------- */

mim_val *mim_std_fs_read(mim_val *path) { return io_result(impl_io_read_file(path)); }

mim_val *mim_std_fs_write(mim_val *path, mim_val *text) {
    mim_val *no = mim_bool(0);
    mim_val *r = io_result(impl_io_write_file(path, text, no));
    mim_release(no);
    return r;
}

mim_val *mim_std_fs_append(mim_val *path, mim_val *text) {
    mim_val *yes = mim_bool(1);
    mim_val *r = io_result(impl_io_write_file(path, text, yes));
    mim_release(yes);
    return r;
}

mim_val *mim_std_fs_size(mim_val *path) { return io_result(impl_io_file_size(path)); }
mim_val *mim_std_fs_copy(mim_val *from, mim_val *to) { return io_result(impl_io_copy_file(from, to)); }
mim_val *mim_std_fs_rename(mim_val *from, mim_val *to) { return io_result(impl_io_rename(from, to)); }
mim_val *mim_std_fs_remove(mim_val *path) { return io_result(impl_io_remove_file(path)); }
mim_val *mim_std_fs_list_dir(mim_val *path) { return io_result(impl_io_list_dir(path)); }
mim_val *mim_std_fs_make_dir(mim_val *path) { return io_result(impl_io_make_dir(path)); }
mim_val *mim_std_fs_exists(mim_val *path) { return impl_exists(path); }
mim_val *mim_std_fs_is_file(mim_val *path) { return impl_is_file(path); }
mim_val *mim_std_fs_is_dir(mim_val *path) { return impl_is_dir(path); }
mim_val *mim_std_fs_current_dir(void) { return impl_current_dir(); }
mim_val *mim_std_fs_temp_dir(void) { return impl_temp_dir(); }

/* ---- path (pure) -------------------------------------------------------- */

mim_val *mim_std_path_join(mim_val *a, mim_val *b) { return impl_path_join(a, b); }
mim_val *mim_std_path_file_name(mim_val *p) { return impl_file_name(p); }
mim_val *mim_std_path_parent(mim_val *p) { return impl_parent(p); }
mim_val *mim_std_path_extension(mim_val *p) { return impl_extension(p); }
mim_val *mim_std_path_stem(mim_val *p) { return impl_stem(p); }

/* ---- io: standard input and error --------------------------------------- */

mim_val *mim_std_io_read_line(void) {
    mim_val *v = impl_io_read_line();
    if (io_failed) {
        mim_release(v);
        return none_val();
    }
    return some_of(v);
}

mim_val *mim_std_io_read_input(void) { return impl_read_input(); }
mim_val *mim_std_io_eprint(mim_val *v) { return impl_eprint(v); }

/* ---- os ----------------------------------------------------------------- */

mim_val *mim_std_os_args(void) { return impl_args(); }

mim_val *mim_std_os_env(mim_val *name) {
    mim_val *v = impl_io_env(name);
    if (io_failed) {
        mim_release(v);
        return none_val();
    }
    return some_of(v);
}

mim_val *mim_std_os_set_env(mim_val *name, mim_val *value) { return io_result(impl_io_set_env(name, value)); }
mim_val *mim_std_os_exit(mim_val *code) { return impl_exit(code); }
mim_val *mim_std_os_pid(void) { return impl_pid(); }

static mim_val *output_of(mim_val *parts);

/* run a program: Ok(Output { status, stdout, stderr }) */
mim_val *mim_std_os_run(mim_val *program, mim_val *args) { return output_of(impl_io_run(program, args)); }

/* a command line through the system shell: /bin/sh -c, or cmd.exe on Windows */
mim_val *mim_std_os_shell(mim_val *command) {
#ifdef _WIN32
    io_clear();
    const char *c = io_path(command, "shell");
    sbuf cmd = {0};
    const char *pre = "cmd.exe /d /s /c \"";
    sb_put(&cmd, pre, strlen(pre));
    sb_put(&cmd, c, strlen(c));
    sb_put(&cmd, "\"", 2); /* (and the terminating zero) */
    mim_val *parts = win_run("cmd.exe", cmd.p);
    free(cmd.p);
    return output_of(parts);
#else
    mim_val *sh = mim_str("/bin/sh", 7);
    mim_val *items[2] = {mim_str("-c", 2), command};
    mim_retain(command);
    mim_val *args = mim_array_lit(2, items);
    mim_release(items[0]);
    mim_release(items[1]);
    mim_val *r = output_of(impl_io_run(sh, args));
    mim_release(sh);
    mim_release(args);
    return r;
#endif
}

static mim_val *output_of(mim_val *parts) {
    if (io_failed) {
        mim_release(parts);
        return err_of(io_err);
    }
    mim_val *f[3];
    for (int i = 0; i < 3; i++) f[i] = scalar_at(parts, i);
    mim_release(parts);
    return ok_of(obj_owned(&mim_desc_os_Output, 3, f));
}

/* ---- time --------------------------------------------------------------- */

mim_val *mim_std_time_now(void) { return impl_now(); }
mim_val *mim_std_time_clock(void) { return impl_clock(); }
mim_val *mim_std_time_sleep(mim_val *d) { return impl_sleep(d); }
mim_val *mim_std_time_format(mim_val *t) { return impl_format_time(t); }

mim_val *mim_std_time_timestamp(mim_val *y, mim_val *mo, mim_val *d, mim_val *h, mim_val *mi, mim_val *s) {
    return impl_timestamp(y, mo, d, h, mi, s);
}

/* [year month day hour minute second nanosecond weekday utc_offset] → DateTime */
static mim_val *date_time_of(mim_val *parts) {
    mim_val *f[9];
    for (int i = 0; i < 9; i++) f[i] = mim_int(((int64_t *)parts->data)[i]);
    mim_release(parts);
    return obj_owned(&mim_desc_time_DateTime, 9, f);
}

mim_val *mim_std_time_utc(mim_val *t) { return date_time_of(impl_time_utc_parts(t)); }
mim_val *mim_std_time_local(mim_val *t) { return date_time_of(impl_time_local_parts(t)); }

/* ---- random ------------------------------------------------------------- */

mim_val *mim_std_random_float(void) { return impl_random(); }
mim_val *mim_std_random_int(mim_val *lo, mim_val *hi) { return impl_random_int(lo, hi); }
mim_val *mim_std_random_seed(mim_val *seed) { return impl_seed_random(seed); }

/* ---- bytes (pure) ------------------------------------------------------- */

static int hexdig(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static mim_val *bytes_n(const void *p, int64_t n) {
    mim_val *r = alloc(MIM_U8, 1, n);
    if (n) memcpy(r->data, p, (size_t)n);
    return r;
}

static void need_bytes(const mim_val *b, const char *fn) {
    if (!b || b->rank != 1 || (b->dtype != MIM_U8 && !(b->dtype == MIM_I64 && b->len == 0))) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected bytes ([u8])", fn);
        mim_panic(buf);
    }
}

/* [72 105] → bytes; every element must be 0 … 255 */
mim_val *mim_std_bytes_from(mim_val *xs) {
    need(xs, "bytes::from");
    if (xs->rank != 1 || IS_HANDLES(xs) || IS_FLOAT(xs->dtype)) mim_panic("bytes::from: expected a list of integers");
    mim_val *r = alloc(MIM_U8, 1, xs->len);
    for (int64_t i = 0; i < xs->len; i++) {
        int64_t v = geti(xs, i);
        if (v < 0 || v > 255) {
            mim_release(r);
            char buf[128];
            snprintf(buf, sizeof buf, "bytes::from: %lld at index %lld is not a byte (0 … 255)", (long long)v, (long long)i);
            mim_panic(buf);
        }
        ((uint8_t *)r->data)[i] = (uint8_t)v;
    }
    return r;
}

/* the UTF-8 bytes of a string */
mim_val *mim_std_bytes_of_text(mim_val *s) {
    if (!s || s->dtype != MIM_CHAR || s->rank != 1) mim_panic("bytes::of_text: expected text (str)");
    return bytes_n(s->data, s->len);
}

static int utf8_valid(const uint8_t *p, int64_t n, int64_t *bad) {
    int64_t i = 0;
    while (i < n) {
        uint8_t c = p[i];
        int64_t len = c < 0x80 ? 1 : (c >> 5) == 6 ? 2 : (c >> 4) == 14 ? 3 : (c >> 3) == 30 ? 4 : 0;
        if (!len || i + len > n) break;
        int ok = 1;
        for (int64_t k = 1; k < len; k++) ok &= (p[i + k] >> 6) == 2;
        if (!ok) break;
        uint32_t cp = len == 1 ? c : len == 2 ? (uint32_t)(c & 0x1F) : len == 3 ? (uint32_t)(c & 0x0F) : (uint32_t)(c & 0x07);
        for (int64_t k = 1; k < len; k++) cp = (cp << 6) | (p[i + k] & 0x3F);
        if ((len == 2 && cp < 0x80) || (len == 3 && cp < 0x800) || (len == 4 && (cp < 0x10000 || cp > 0x10FFFF)) || (cp >= 0xD800 && cp <= 0xDFFF)) break;
        i += len;
    }
    *bad = i;
    return i == n;
}

/* bytes as text: Err when they are not valid UTF-8 */
mim_val *mim_std_bytes_to_text(mim_val *b) {
    need_bytes(b, "bytes::to_text");
    int64_t bad;
    if (!utf8_valid((const uint8_t *)b->data, b->len, &bad)) {
        char buf[96];
        snprintf(buf, sizeof buf, "not valid UTF-8 at byte %lld", (long long)bad);
        return err_of(buf);
    }
    return ok_of(mim_str((const char *)b->data, b->len));
}

mim_val *mim_std_bytes_hex(mim_val *b) {
    need_bytes(b, "bytes::hex");
    static const char d[] = "0123456789abcdef";
    mim_val *r = alloc(MIM_CHAR, 1, b->len * 2);
    for (int64_t i = 0; i < b->len; i++) {
        uint8_t c = ((uint8_t *)b->data)[i];
        ((char *)r->data)[2 * i] = d[c >> 4];
        ((char *)r->data)[2 * i + 1] = d[c & 15];
    }
    return r;
}

mim_val *mim_std_bytes_from_hex(mim_val *s) {
    if (!s || s->dtype != MIM_CHAR || s->rank != 1) mim_panic("bytes::from_hex: expected text (str)");
    if (s->len % 2) return err_of("hex text must have an even number of digits");
    mim_val *r = alloc(MIM_U8, 1, s->len / 2);
    const char *p = (const char *)s->data;
    for (int64_t i = 0; i < s->len / 2; i++) {
        int h = hexdig(p[2 * i]), l = hexdig(p[2 * i + 1]);
        if (h < 0 || l < 0) {
            mim_release(r);
            char buf[64];
            snprintf(buf, sizeof buf, "not a hex digit at %lld", (long long)(h < 0 ? 2 * i : 2 * i + 1));
            return err_of(buf);
        }
        ((uint8_t *)r->data)[i] = (uint8_t)(h * 16 + l);
    }
    return ok_of(r);
}

static const char B64[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

mim_val *mim_std_bytes_base64(mim_val *b) {
    need_bytes(b, "bytes::base64");
    const uint8_t *p = (const uint8_t *)b->data;
    int64_t n = b->len, out = (n + 2) / 3 * 4;
    mim_val *r = alloc(MIM_CHAR, 1, out);
    char *o = (char *)r->data;
    for (int64_t i = 0, k = 0; i < n; i += 3) {
        uint32_t v = (uint32_t)p[i] << 16 | (i + 1 < n ? (uint32_t)p[i + 1] << 8 : 0) | (i + 2 < n ? p[i + 2] : 0);
        o[k++] = B64[v >> 18 & 63];
        o[k++] = B64[v >> 12 & 63];
        o[k++] = i + 1 < n ? B64[v >> 6 & 63] : '=';
        o[k++] = i + 2 < n ? B64[v & 63] : '=';
    }
    return r;
}

mim_val *mim_std_bytes_from_base64(mim_val *s) {
    if (!s || s->dtype != MIM_CHAR || s->rank != 1) mim_panic("bytes::from_base64: expected text (str)");
    const char *p = (const char *)s->data;
    sbuf b = {0};
    uint32_t acc = 0;
    int bits = 0;
    for (int64_t i = 0; i < s->len; i++) {
        char c = p[i];
        if (c == '=' || c == '\n' || c == '\r') continue;
        const char *at = strchr(B64, c);
        if (!at || !c) {
            free(b.p);
            char buf[64];
            snprintf(buf, sizeof buf, "not base64 at character %lld", (long long)i);
            return err_of(buf);
        }
        acc = acc << 6 | (uint32_t)(at - B64);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            char byte = (char)(acc >> bits & 0xFF);
            sb_put(&b, &byte, 1);
        }
    }
    mim_val *r = bytes_n(b.p, (int64_t)b.n);
    free(b.p);
    return ok_of(r);
}

/* ---- fs: bytes ---------------------------------------------------------- */

mim_val *mim_std_fs_read_bytes(mim_val *path) {
    mim_val *r = mim_std_fs_read(path);
    if (r->desc != &mim_desc_Ok) return r;
    mim_val *t = ((mim_val **)r->data)[0];
    mim_val *b = bytes_n(t->data, t->len);
    mim_release(r);
    return ok_of(b);
}

mim_val *mim_std_fs_write_bytes(mim_val *path, mim_val *b) {
    need_bytes(b, "fs::write_bytes");
    mim_val *t = mim_str((const char *)b->data, b->len); /* a str holds any bytes */
    mim_val *r = mim_std_fs_write(path, t);
    mim_release(t);
    return r;
}

/* ---- chan: channels between tasks ----------------------------------------
 * An unbounded queue: `send` never waits, `receive` waits for a value or for
 * the channel to be closed and drained. Values are immutable and reference
 * counted atomically, so handing one to another thread is safe. */

extern const mim_desc mim_desc_tuple, mim_desc_chan_Sender, mim_desc_chan_Receiver;

typedef struct {
#if MIM_THREADS
    pthread_mutex_t m;
    pthread_cond_t c;     /* a value arrived (or closed) */
    pthread_cond_t space; /* a bounded channel has room again (or closed) */
#endif
    mim_val **q;
    int64_t head, n, cap;
    int64_t limit; /* the most values it holds; 0: unbounded */
    int closed;
#if MIM_GREEN
    mim_co *waiters, *waiters_tail;   /* coroutines waiting in receive */
    mim_co *senders, *senders_tail;   /* coroutines waiting in send */
#endif
} chan_t;

static chan_t **ch_tab;
static int64_t ch_n, ch_cap;
#if MIM_THREADS
static pthread_mutex_t ch_tab_lock = PTHREAD_MUTEX_INITIALIZER;
#define CH_LOCK(x) pthread_mutex_lock(x)
#define CH_UNLOCK(x) pthread_mutex_unlock(x)
#else
#define CH_LOCK(x) ((void)0)
#define CH_UNLOCK(x) ((void)0)
#endif

static chan_t *chan_of(mim_val *end, const char *what) {
    if (!end || end->dtype != MIM_OBJ || (end->desc != &mim_desc_chan_Sender && end->desc != &mim_desc_chan_Receiver)) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected a channel end", what);
        mim_panic(buf);
    }
    int64_t id = geti(((mim_val **)end->data)[0], 0);
    CH_LOCK(&ch_tab_lock);
    chan_t *c = id >= 0 && id < ch_n ? ch_tab[id] : NULL;
    CH_UNLOCK(&ch_tab_lock);
    if (!c) mim_panic("not a channel");
    return c;
}

static mim_val *chan_make(int64_t limit) {
    chan_t *c = (chan_t *)calloc(1, sizeof(chan_t));
    c->limit = limit;
#if MIM_THREADS
    pthread_mutex_init(&c->m, NULL);
    pthread_cond_init(&c->c, NULL);
    pthread_cond_init(&c->space, NULL);
#endif
    CH_LOCK(&ch_tab_lock);
    if (ch_n == ch_cap) {
        ch_cap = ch_cap ? ch_cap * 2 : 16;
        ch_tab = (chan_t **)realloc(ch_tab, sizeof(chan_t *) * (size_t)ch_cap);
    }
    int64_t id = ch_n++;
    ch_tab[id] = c;
    CH_UNLOCK(&ch_tab_lock);
    mim_val *a = mim_int(id), *b = mim_int(id);
    mim_val *ends[2] = {obj_owned(&mim_desc_chan_Sender, 1, &a), obj_owned(&mim_desc_chan_Receiver, 1, &b)};
    return obj_owned(&mim_desc_tuple, 2, ends);
}

/* (Sender, Receiver) of a new channel; the schema argument (T) is unused */
mim_val *mim_std_chan_new(mim_val *schema) {
    (void)schema;
    return chan_make(0);
}

/* a channel that holds at most `capacity` values: send waits while it is full */
mim_val *mim_std_chan_bounded(mim_val *schema, mim_val *capacity) {
    (void)schema;
    int64_t n = geti(capacity, 0);
    if (n < 1) mim_panic("chan::bounded: the capacity must be at least 1");
    return chan_make(n);
}

#if MIM_GREEN
/* put co at the end of a waiter list */
static void co_list_add(mim_co **head, mim_co **tail, mim_co *co) {
    co->next = NULL;
    if (*tail) (*tail)->next = co;
    else *head = co;
    *tail = co;
}

static mim_co *co_list_take(mim_co **head, mim_co **tail) {
    mim_co *co = *head;
    if (co) {
        *head = co->next;
        if (!*head) *tail = NULL;
    }
    return co;
}

/* wake every coroutine of a list (after the lock is released) */
static void co_list_wake(mim_co *co) {
    while (co) {
        mim_co *n = co->next;
        co_ready(co);
        co = n;
    }
}
#endif

/* after a value left the channel (lock held): one waiting sender may go on.
 * Returns a coroutine to wake once the lock is released. */
static void *chan_room(chan_t *c) {
    if (!c->limit) return NULL;
#if MIM_THREADS
    pthread_cond_signal(&c->space);
#endif
#if MIM_GREEN
    return co_list_take(&c->senders, &c->senders_tail);
#else
    return NULL;
#endif
}

static void chan_room_wake(void *co) {
#if MIM_GREEN
    if (co) co_ready((mim_co *)co);
#else
    (void)co;
#endif
}

#if MIM_GREEN
static void chan_send_park(mim_co *co, void *arg);
#endif

/* true when sent; false when the channel is closed */
mim_val *mim_std_chan_send(mim_val *tx, mim_val *v) {
    chan_t *c = chan_of(tx, "chan::send");
    need(v, "chan::send");
    CH_LOCK(&c->m);
    while (c->limit && c->n >= c->limit && !c->closed) {
#if MIM_GREEN
        if (t_co) {
            CH_UNLOCK(&c->m);
            co_park(chan_send_park, c);
            CH_LOCK(&c->m);
            continue;
        }
#endif
#if MIM_THREADS
        pthread_cond_wait(&c->space, &c->m);
#else
        /* without threads: deferred tasks may make room; a task cannot
         * wait mid-way there, so when none is left the channel grows */
        if (!lazy_run_one()) break;
#endif
    }
    if (c->closed) {
        CH_UNLOCK(&c->m);
        return mim_bool(0);
    }
    if (c->n == c->cap) {
        int64_t cap = c->cap ? c->cap * 2 : 16;
        mim_val **q = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
        for (int64_t k = 0; k < c->n; k++) q[k] = c->q[(c->head + k) % c->cap];
        free(c->q);
        c->q = q;
        c->head = 0;
        c->cap = cap;
    }
    mim_retain(v);
    c->q[(c->head + c->n) % c->cap] = v;
    c->n++;
#if MIM_THREADS
    pthread_cond_signal(&c->c);
#endif
#if MIM_GREEN
    mim_co *w = co_list_take(&c->waiters, &c->waiters_tail);
    CH_UNLOCK(&c->m);
    if (w) co_ready(w);
#else
    CH_UNLOCK(&c->m);
#endif
    return mim_bool(1);
}

#if MIM_GREEN
/* runs once the receiving coroutine is off its stack */
static void chan_park(mim_co *co, void *arg) {
    chan_t *c = (chan_t *)arg;
    CH_LOCK(&c->m);
    int now = c->n > 0 || c->closed;
    if (!now) co_list_add(&c->waiters, &c->waiters_tail, co);
    CH_UNLOCK(&c->m);
    if (now) co_ready(co);
}

/* runs once the sending coroutine is off its stack */
static void chan_send_park(mim_co *co, void *arg) {
    chan_t *c = (chan_t *)arg;
    CH_LOCK(&c->m);
    int now = c->n < c->limit || c->closed;
    if (!now) co_list_add(&c->senders, &c->senders_tail, co);
    CH_UNLOCK(&c->m);
    if (now) co_ready(co);
}
#endif

static mim_val *chan_take(chan_t *c) {
    mim_val *v = c->q[c->head];
    c->head = (c->head + 1) % c->cap;
    c->n--;
    return v; /* the queue's reference moves to the caller */
}

/* Some(next value), waiting for one; None once closed and drained */
mim_val *mim_std_chan_receive(mim_val *rx) {
    chan_t *c = chan_of(rx, "chan::receive");
    CH_LOCK(&c->m);
#if MIM_THREADS
#if MIM_GREEN
    while (t_co && c->n == 0 && !c->closed) {
        CH_UNLOCK(&c->m);
        co_park(chan_park, c);
        CH_LOCK(&c->m);
    }
#endif
    while (c->n == 0 && !c->closed) pthread_cond_wait(&c->c, &c->m);
#else
    /* without threads: let deferred tasks run until there is something */
    while (c->n == 0 && !c->closed && lazy_run_one()) {
    }
    if (c->n == 0 && !c->closed) mim_panic("chan::receive: the channel is empty and no task is left that could send to it");
#endif
    mim_val *v = c->n ? chan_take(c) : NULL;
    void *room = v ? chan_room(c) : NULL;
    CH_UNLOCK(&c->m);
    chan_room_wake(room);
    return v ? some_of(v) : none_val();
}

/* Some(value) if one is waiting, else None (never waits) */
mim_val *mim_std_chan_try_receive(mim_val *rx) {
    chan_t *c = chan_of(rx, "chan::try_receive");
    CH_LOCK(&c->m);
    mim_val *v = c->n ? chan_take(c) : NULL;
    void *room = v ? chan_room(c) : NULL;
    CH_UNLOCK(&c->m);
    chan_room_wake(room);
    return v ? some_of(v) : none_val();
}

/* no more values: receivers get the rest, then None */
mim_val *mim_std_chan_close(mim_val *tx) {
    chan_t *c = chan_of(tx, "chan::close");
    CH_LOCK(&c->m);
    c->closed = 1;
#if MIM_THREADS
    pthread_cond_broadcast(&c->c);
    pthread_cond_broadcast(&c->space);
#endif
#if MIM_GREEN
    mim_co *w = c->waiters, *snd = c->senders;
    c->waiters = c->waiters_tail = NULL;
    c->senders = c->senders_tail = NULL;
    CH_UNLOCK(&c->m);
    co_list_wake(w);
    co_list_wake(snd);
#else
    CH_UNLOCK(&c->m);
#endif
    return NULL;
}
