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
