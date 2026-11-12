/* Program basics: arguments, environment, files, directories, paths, stdin.
 * Included at the end of mimic_rt.c (same translation unit).
 *
 * Functions declared `extern "mimic-io"` in the prelude have side effects:
 * the compiler never runs them in parallel or out of order. They report
 * failure through a per-thread "last error" that the prelude's Mimic
 * wrappers turn into `Result` / `Option` values:
 *
 *     pub fn read_file(path: str) -> Result[str, str] {
 *         let s = io_read_file(path)
 *         if io_ok() { Ok(s) } else { Err(io_error()) }
 *     }
 *
 * Path helpers (`path_join`, `file_name`, …) are pure `extern "mimic"`. */

#include <dirent.h>
#include <sys/stat.h>
#include <fcntl.h>
#ifndef _WIN32
#include <unistd.h>
#endif

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

mim_val *mim_std_io_ok(void) { return mim_bool(!io_failed); }
mim_val *mim_std_io_error(void) { return mim_from_cstr(io_err); }

/* ---- program ------------------------------------------------------------ */

/* the command-line arguments, without the program's own name */
mim_val *mim_std_args(void) {
    int64_t n = g_argc > 1 ? g_argc - 1 : 0;
    if (n == 0) return alloc(MIM_BOXED, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    for (int64_t i = 0; i < n; i++) tmp[i] = mim_from_cstr(g_argv[i + 1]);
    mim_val *r = mim_array_lit(n, tmp);
    for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
    free(tmp);
    return r;
}

mim_val *mim_std_io_env(mim_val *name) {
    io_clear();
    const char *v = getenv(io_path(name, "env"));
    if (!v) {
        io_fail_msg("not set");
        return mim_str("", 0);
    }
    return mim_from_cstr(v);
}

mim_val *mim_std_io_set_env(mim_val *name, mim_val *value) {
    io_clear();
#ifdef _WIN32
    if (_putenv_s(io_path(name, "set_env"), io_path(value, "set_env")) != 0) io_fail("set_env", io_path(name, "set_env"));
#else
    if (setenv(io_path(name, "set_env"), io_path(value, "set_env"), 1) != 0) io_fail("set_env", io_path(name, "set_env"));
#endif
    return mim_bool(!io_failed);
}

/* print to standard error (a newline follows) */
mim_val *mim_std_eprint(mim_val *v) {
    fflush(stdout);
    sbuf b = {0};
    fmt_val(&b, v, 0);
    if (b.n) fwrite(b.p, 1, b.n, stderr);
    fputc('\n', stderr);
    free(b.p);
    return NULL;
}

/* leave the program now with an exit status */
mim_val *mim_std_exit(mim_val *code) {
    fflush(stdout);
    fflush(stderr);
    exit((int)(code && code->rank == 0 && code->dtype != MIM_F64 ? geti(code, 0) : 1));
    return NULL;
}

/* ---- standard input ----------------------------------------------------- */

/* one line without its newline; at end of input: error "end of input" */
mim_val *mim_std_io_read_line(void) {
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
mim_val *mim_std_read_input(void) {
    sbuf b = {0};
    char buf[65536];
    size_t n;
    while ((n = fread(buf, 1, sizeof buf, stdin)) > 0) sb_put(&b, buf, n);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

/* ---- files -------------------------------------------------------------- */

mim_val *mim_std_io_read_file(mim_val *path) {
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
mim_val *mim_std_io_write_file(mim_val *path, mim_val *text, mim_val *append) {
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

mim_val *mim_std_exists(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "exists"), &st));
}

mim_val *mim_std_is_dir(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "is_dir"), &st) && S_ISDIR(st.st_mode));
}

mim_val *mim_std_is_file(mim_val *path) {
    struct stat st;
    return mim_bool(stat_of(io_path(path, "is_file"), &st) && S_ISREG(st.st_mode));
}

mim_val *mim_std_io_file_size(mim_val *path) {
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
mim_val *mim_std_io_remove_file(mim_val *path) {
    io_clear();
    const char *p = io_path(path, "remove");
    struct stat st;
    int ok = stat_of(p, &st) && (S_ISDIR(st.st_mode) ? rmdir(p) == 0 : unlink(p) == 0);
    if (!ok) io_fail("cannot remove", p);
    return mim_bool(ok);
}

mim_val *mim_std_io_rename(mim_val *from, mim_val *to) {
    io_clear();
    const char *a = io_path(from, "rename"), *b = io_path(to, "rename");
    if (rename(a, b) != 0) io_fail("cannot rename", a);
    return mim_bool(!io_failed);
}

mim_val *mim_std_io_copy_file(mim_val *from, mim_val *to) {
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
mim_val *mim_std_io_list_dir(mim_val *path) {
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
mim_val *mim_std_io_make_dir(mim_val *path) {
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

mim_val *mim_std_current_dir(void) {
    char buf[4096];
    if (!getcwd(buf, sizeof buf)) return mim_str(".", 1);
    return mim_from_cstr(buf);
}

mim_val *mim_std_temp_dir(void) {
    const char *t = getenv("TMPDIR");
    if (!t || !*t) t = "/tmp";
    size_t n = strlen(t);
    while (n > 1 && t[n - 1] == '/') n--; /* no trailing slash */
    return mim_str(t, (int64_t)n);
}

/* ---- paths (pure) ------------------------------------------------------- */

/* "a/b" + "c" → "a/b/c"; an absolute second part replaces the first */
mim_val *mim_std_path_join(mim_val *a, mim_val *b) {
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
mim_val *mim_std_file_name(mim_val *path) {
    const char *p = io_path(path, "file_name");
    size_t n = strlen(p);
    while (n > 1 && p[n - 1] == '/') n--;
    size_t s = n;
    while (s > 0 && p[s - 1] != '/') s--;
    return mim_str(p + s, (int64_t)(n - s));
}

/* everything before the last component: "a/b/c.txt" → "a/b"; "c" → "" */
mim_val *mim_std_parent(mim_val *path) {
    const char *p = io_path(path, "parent");
    size_t n = strlen(p);
    while (n > 1 && p[n - 1] == '/') n--;
    while (n > 0 && p[n - 1] != '/') n--;
    while (n > 1 && p[n - 1] == '/') n--;
    return mim_str(p, (int64_t)n);
}

/* "c.tar.gz" → "gz"; no dot (or a leading dot only) → "" */
mim_val *mim_std_extension(mim_val *path) {
    mim_val *f = mim_std_file_name(path);
    const char *s = (const char *)f->data;
    const char *dot = strrchr(s, '.');
    mim_val *r = dot && dot != s ? mim_from_cstr(dot + 1) : mim_str("", 0);
    mim_release(f);
    return r;
}

/* the file name without its extension: "a/report.pdf" → "report" */
mim_val *mim_std_stem(mim_val *path) {
    mim_val *f = mim_std_file_name(path);
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
#if defined(_WIN32) || defined(__wasi__) || (defined(__APPLE__) && (TARGET_OS_TV || TARGET_OS_WATCH))
#define MIM_NO_SPAWN 1
#endif
#ifndef MIM_NO_SPAWN
#include <spawn.h>
#include <poll.h>
#include <sys/wait.h>
extern char **environ;
#endif

/* ---- clocks ------------------------------------------------------------- */

/* wall-clock time: nanoseconds since 1970-01-01 00:00:00 UTC */
mim_val *mim_std_now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return mim_int((int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec);
}

/* a monotonic clock in nanoseconds: for measuring how long things take */
mim_val *mim_std_clock(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return mim_int((int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec);
}

mim_val *mim_std_sleep(mim_val *d) {
    int64_t ns = d && d->rank == 0 ? (d->dtype == MIM_F64 ? (int64_t)getf(d, 0) : geti(d, 0)) : 0;
    if (ns <= 0) return NULL;
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

mim_val *mim_std_time_utc_parts(mim_val *t) { return date_parts(geti(t, 0), 0); }

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

mim_val *mim_std_time_local_parts(mim_val *t) {
    int64_t v = geti(t, 0);
    return date_parts(v, local_offset(v));
}

/* the instant of a UTC calendar date and time (ns since the epoch) */
mim_val *mim_std_timestamp(mim_val *y, mim_val *mo, mim_val *d, mim_val *h, mim_val *mi, mim_val *s) {
    int64_t days = days_from_civil(geti(y, 0), geti(mo, 0), geti(d, 0));
    int64_t secs = days * 86400 + geti(h, 0) * 3600 + geti(mi, 0) * 60 + geti(s, 0);
    return mim_int(secs * 1000000000);
}

/* ISO 8601 / RFC 3339 in UTC: 2026-11-12T02:40:05Z (milliseconds when present) */
mim_val *mim_std_format_time(mim_val *t) {
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

static uint64_t rng_next(void) {
    if (!rng_ready) {
        uint64_t seed = 0;
#if defined(_WIN32)
        seed = (uint64_t)time(NULL) ^ (uint64_t)(uintptr_t)&seed;
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
mim_val *mim_std_random(void) { return mim_float((double)(rng_next() >> 11) * 0x1.0p-53); }

/* an integer in [lo, hi), without modulo bias */
mim_val *mim_std_random_int(mim_val *lo, mim_val *hi) {
    int64_t a = geti(lo, 0), b = geti(hi, 0);
    if (b <= a) mim_panic("random_int(lo, hi) needs lo < hi");
    uint64_t span = (uint64_t)b - (uint64_t)a, limit = UINT64_MAX - UINT64_MAX % span, x;
    do x = rng_next(); while (x >= limit);
    return mim_int((int64_t)((uint64_t)a + x % span));
}

/* make this thread's random numbers repeatable */
mim_val *mim_std_seed_random(mim_val *seed) {
    rng_seed((uint64_t)geti(seed, 0));
    return NULL;
}

/* ---- processes ---------------------------------------------------------- */

mim_val *mim_std_pid(void) {
#if defined(_WIN32) || defined(__wasi__)
    return mim_int(0);
#else
    return mim_int((int64_t)getpid());
#endif
}

/* run a program (looked up in PATH) with arguments; stdin is empty.
 * [status stdout stderr]; a signal gives status 128 + signal number */
mim_val *mim_std_io_run(mim_val *program, mim_val *argv_list) {
    io_clear();
#ifdef MIM_NO_SPAWN
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
