/* Mimic's standard-library intrinsics: strings, lists and maps.
 *
 * Declared in the prelude as `extern "mimic" { fn split(s: str, sep: str) -> [str] }`;
 * the compiler calls `mim_std_<name>`. Every function borrows its arguments
 * and returns an owned value. Compiled as part of mimic_rt.c (it uses the
 * allocator, formatting and kernels directly). */

/* ==== helpers ============================================================== */

static int is_str(const mim_val *v) { return v && v->dtype == MIM_CHAR && v->rank == 1; }

static const char *str_arg(const mim_val *v, const char *fn) {
    if (!is_str(v)) {
        char buf[160];
        snprintf(buf, sizeof buf, "%s: expected a str, found %s", fn, dtname(v));
        mim_panic(buf);
    }
    return (const char *)v->data;
}

static int64_t int_arg(mim_val *v, const char *fn) {
    if (!v || v->rank != 0 || (v->dtype != MIM_I64 && v->dtype != MIM_BOOL)) {
        char buf[160];
        snprintf(buf, sizeof buf, "%s: expected an integer, found %s", fn, dtname(v));
        mim_panic(buf);
    }
    return geti(v, 0);
}

/* A list from owned values, which it takes over (no retain/release pairs):
 * numbers become a numeric vector, as in an array literal. */
static mim_val *list_take(int64_t n, mim_val **xs) {
    /* one handle makes a list of handles: a first one settles it without
     * reading the rest (a list of records: no pass over all of them) */
    int boxed = n > 0 && (xs[0]->rank != 0 || IS_HANDLES(xs[0])), dt = -1;
    for (int64_t k = 0; k < n && !boxed; k++) {
        if (xs[k]->rank != 0 || IS_HANDLES(xs[k])) boxed = 1;
        else dt = dt == -1 ? xs[k]->dtype : join_dtype(dt, xs[k]->dtype);
    }
    if (!boxed) {
        mim_val *l = mim_array_lit(n, xs);
        for (int64_t k = 0; k < n; k++) mim_release(xs[k]);
        return l;
    }
    mim_val *v = alloc(MIM_BOXED, 1, n);
    memcpy(v->data, xs, sizeof(mim_val *) * (size_t)n);
    return v;
}

/* ==== strings ============================================================== */

mim_val *mim_std_split(mim_val *s, mim_val *sep) {
    const char *a = str_arg(s, "split"), *d = str_arg(sep, "split");
    int64_t n = s->len, m = sep->len;
    int64_t cap = 8, k = 0;
    mim_val **parts = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
    int64_t start = 0;
    for (int64_t i = 0; i <= n; i++) {
        int at_sep = m > 0 && i + m <= n && memcmp(a + i, d, (size_t)m) == 0;
        int one_char = m == 0 && i < n; /* split(s, "") → characters (bytes of UTF-8 kept together) */
        if (one_char) {
            int64_t len = 1;
            unsigned char c = (unsigned char)a[i];
            if (c >= 0xF0) len = 4; else if (c >= 0xE0) len = 3; else if (c >= 0xC0) len = 2;
            if (i + len > n) len = n - i;
            if (k == cap) parts = (mim_val **)realloc(parts, sizeof(mim_val *) * (size_t)(cap *= 2));
            parts[k++] = mim_str(a + i, len);
            i += len - 1;
            continue;
        }
        if (m == 0) break;
        if (at_sep || i == n) {
            if (k == cap) parts = (mim_val **)realloc(parts, sizeof(mim_val *) * (size_t)(cap *= 2));
            parts[k++] = mim_str(a + start, i - start);
            if (at_sep) {
                i += m - 1;
                start = i + 1;
            }
        }
    }
    mim_val *r = k == 0 ? alloc(MIM_BOXED, 1, 0) : list_take(k, parts);
    free(parts);
    return r;
}

mim_val *mim_std_chars(mim_val *s) {
    mim_val *empty = mim_str("", 0);
    mim_val *r = mim_std_split(s, empty);
    mim_release(empty);
    return r;
}

/* lines(s): split at "\n" (a "\r" before it is dropped too); a final newline
 * ends the last line instead of starting an empty one */
mim_val *mim_std_lines(mim_val *s) {
    const char *p = str_arg(s, "lines");
    int64_t n = s->len, count = 0;
    if (n > 0 && p[n - 1] == '\n') n--;
    if (n == 0 && s->len == 0) return alloc(MIM_BOXED, 1, 0);
    for (int64_t i = 0; i < n; i++) count += p[i] == '\n';
    count++;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)count);
    int64_t start = 0, k = 0;
    for (int64_t i = 0; i <= n; i++) {
        if (i == n || p[i] == '\n') {
            int64_t end = i;
            if (end > start && p[end - 1] == '\r') end--;
            tmp[k++] = mim_str(p + start, end - start);
            start = i + 1;
        }
    }
    mim_val *r = mim_array_lit(count, tmp);
    for (int64_t i = 0; i < count; i++) mim_release(tmp[i]);
    free(tmp);
    return r;
}

mim_val *mim_std_join(mim_val *xs, mim_val *sep) {
    const char *d = str_arg(sep, "join");
    need(xs, "join");
    if (is_str(xs)) {
        mim_retain(xs);
        return xs;
    }
    if (xs->rank == 0 || (xs->dtype != MIM_BOXED && xs->len > 0)) mim_panic("join: expected a list of str");
    sbuf b = {0};
    mim_val **h = (mim_val **)xs->data;
    for (int64_t i = 0; i < xs->len; i++) {
        if (i) sb_put(&b, d, (size_t)sep->len);
        sb_put(&b, str_arg(h[i], "join"), (size_t)h[i]->len);
    }
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

static int is_space(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f'; }

static mim_val *trim_impl(mim_val *s, int left, int right) {
    const char *a = str_arg(s, "trim");
    int64_t lo = 0, hi = s->len;
    while (left && lo < hi && is_space(a[lo])) lo++;
    while (right && hi > lo && is_space(a[hi - 1])) hi--;
    return mim_str(a + lo, hi - lo);
}

mim_val *mim_std_trim(mim_val *s) { return trim_impl(s, 1, 1); }
mim_val *mim_std_trim_start(mim_val *s) { return trim_impl(s, 1, 0); }
mim_val *mim_std_trim_end(mim_val *s) { return trim_impl(s, 0, 1); }

static int64_t find_bytes(const char *a, int64_t n, const char *b, int64_t m, int64_t from) {
    if (m == 0) return from <= n ? from : -1;
    for (int64_t i = from; i + m <= n; i++)
        if (memcmp(a + i, b, (size_t)m) == 0) return i;
    return -1;
}

static int vals_equal(const mim_val *a, const mim_val *b);

/* contains(text, part) for strings; contains(list, x) for lists */
mim_val *mim_std_contains(mim_val *hay, mim_val *needle) {
    need(hay, "contains");
    if (is_str(hay) && is_str(needle)) return mim_bool(find_bytes((const char *)hay->data, hay->len, (const char *)needle->data, needle->len, 0) >= 0);
    if (hay->rank == 0) mim_panic("contains: expected a str or a list");
    for (int64_t i = 0; i < hay->len; i++) {
        mim_val *e = scalar_at(hay, i);
        int eq = vals_equal(e, needle);
        mim_release(e);
        if (eq) return mim_bool(1);
    }
    return mim_bool(0);
}

mim_val *mim_std_starts_with(mim_val *s, mim_val *p) {
    str_arg(s, "starts_with");
    str_arg(p, "starts_with");
    return mim_bool(p->len <= s->len && memcmp(s->data, p->data, (size_t)p->len) == 0);
}

mim_val *mim_std_ends_with(mim_val *s, mim_val *p) {
    str_arg(s, "ends_with");
    str_arg(p, "ends_with");
    return mim_bool(p->len <= s->len && memcmp((char *)s->data + s->len - p->len, p->data, (size_t)p->len) == 0);
}

/* byte index of the first occurrence, or -1 (the prelude's `find` wraps it in Option) */
mim_val *mim_std_find_index(mim_val *s, mim_val *p) {
    str_arg(s, "find");
    str_arg(p, "find");
    return mim_int(find_bytes((const char *)s->data, s->len, (const char *)p->data, p->len, 0));
}

mim_val *mim_std_replace(mim_val *s, mim_val *from, mim_val *to) {
    const char *a = str_arg(s, "replace"), *f = str_arg(from, "replace"), *t = str_arg(to, "replace");
    if (from->len == 0) {
        mim_retain(s);
        return s;
    }
    sbuf b = {0};
    int64_t i = 0;
    for (;;) {
        int64_t j = find_bytes(a, s->len, f, from->len, i);
        if (j < 0) break;
        sb_put(&b, a + i, (size_t)(j - i));
        sb_put(&b, t, (size_t)to->len);
        i = j + from->len;
    }
    sb_put(&b, a + i, (size_t)(s->len - i));
    mim_val *r = mim_str(b.p, (int64_t)b.n);
    free(b.p);
    return r;
}

static mim_val *map_bytes(mim_val *s, int upper, const char *fn) {
    const char *a = str_arg(s, fn);
    mim_val *r = mim_str(a, s->len);
    char *d = (char *)r->data;
    for (int64_t i = 0; i < s->len; i++) {
        if (upper && d[i] >= 'a' && d[i] <= 'z') d[i] = (char)(d[i] - 32);
        if (!upper && d[i] >= 'A' && d[i] <= 'Z') d[i] = (char)(d[i] + 32);
    }
    return r;
}

mim_val *mim_std_upper(mim_val *s) { return map_bytes(s, 1, "upper"); }
mim_val *mim_std_lower(mim_val *s) { return map_bytes(s, 0, "lower"); }

mim_val *mim_std_repeat(mim_val *s, mim_val *n) {
    const char *a = str_arg(s, "repeat");
    int64_t k = int_arg(n, "repeat");
    if (k < 0) mim_panic("repeat: the count cannot be negative");
    mim_val *r = alloc(MIM_CHAR, 1, s->len * k);
    for (int64_t i = 0; i < k; i++) memcpy((char *)r->data + i * s->len, a, (size_t)s->len);
    ((char *)r->data)[s->len * k] = 0;
    return r;
}

/* string(x): any value as text, the way print shows it */
mim_val *mim_std_string(mim_val *v) {
    if (is_str(v)) {
        mim_retain(v);
        return v;
    }
    sbuf b = {0};
    fmt_val(&b, v, 0);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

static int parse_i64(const mim_val *s, int64_t *out) {
    if (!is_str(s) || s->len == 0) return 0;
    char *end;
    errno = 0;
    long long x = strtoll((const char *)s->data, &end, 10);
    if (errno || end != (const char *)s->data + s->len) return 0;
    *out = x;
    return 1;
}

static int parse_f64(const mim_val *s, double *out) {
    if (!is_str(s) || s->len == 0) return 0;
    char *end;
    double x = strtod((const char *)s->data, &end);
    if (end != (const char *)s->data + s->len) return 0;
    *out = x;
    return 1;
}

mim_val *mim_std_is_int(mim_val *s) {
    int64_t x;
    return mim_bool(parse_i64(s, &x));
}

mim_val *mim_std_to_int(mim_val *s) {
    int64_t x;
    if (!parse_i64(s, &x)) mim_panic("to_int: not an integer");
    return mim_int(x);
}

mim_val *mim_std_is_float(mim_val *s) {
    double x;
    return mim_bool(parse_f64(s, &x));
}

mim_val *mim_std_to_float(mim_val *s) {
    double x;
    if (!parse_f64(s, &x)) mim_panic("to_float: not a number");
    return mim_float(x);
}

/* ==== lists ================================================================ */

/* a list with x appended (a copy: values are immutable) */
/* ==== owned list updates (push, concat, put on a list held once) ==========
 * `{ s with out: push(s.out, x) }` on a record held once detaches the field
 * and hands the list over (codegen: the steal in `with`). With the only
 * reference, a list of plain elements grows or changes in place; growth past
 * its block moves it once to a malloc'd block with room to spare, then
 * realloc grows it (amortised O(1) per push, as a C vector). Anything else
 * takes the usual copying path; the result is the same either way. */
#if defined(__APPLE__)
#include <malloc/malloc.h>
static size_t block_usable(void *p) { return malloc_size(p); }
#elif defined(_WIN32)
#include <malloc.h>
static size_t block_usable(void *p) { return _msize(p); }
#else
#include <malloc.h>
static size_t block_usable(void *p) { return malloc_usable_size(p); }
#endif

mim_val *mim_std_push(mim_val *xs, mim_val *x);
mim_val *mim_std_put(mim_val *m, mim_val *k, mim_val *v);
mim_val *mim_std_put_own(mim_val *xs, mim_val *k, mim_val *v);

static int own_plain(const mim_val *xs) {
    return xs && xs->rank == 1 && !IS_HANDLES(xs) && xs->dtype != MIM_MAP && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1;
}

/* xs (held once, plain elements) with room for n elements, len set to n;
 * may move */
/* A list grown here remembers its room (payload bytes) in `desc`, which a
 * plain list does not use otherwise (records, functions, maps and matrices
 * do); NULL: not known, ask the allocator. Copies of a list never copy its
 * header, so the number stays with its block. */
#define ROOM_OF(v) ((size_t)(uintptr_t)(v)->desc)
static mim_val *grow_owned(mim_val *xs, int64_t n) {
    int dt = xs->dtype;
    size_t es = elsize(dt);
    size_t old = payload_bytes(dt, xs->len), need = payload_bytes(dt, n);
    int c_old = size_class(old);
    if (c_old < SIZE_CLASSES && size_class(need) == c_old && !xs->desc) {
        /* the same pooled block has room */
    } else if (c_old >= SIZE_CLASSES && (xs->desc ? ROOM_OF(xs) : block_usable(xs) - sizeof(mim_val)) >= need) {
        /* the malloc'd block has room */
    } else {
        size_t cap = need * 2 < 4096 ? 4096 : need * 2;
        if (c_old >= SIZE_CLASSES || xs->desc) {
            mim_val *r = (mim_val *)realloc(xs, sizeof(mim_val) + cap);
            if (!r) mim_panic("out of memory");
            xs = r;
        } else {
            mim_val *r = (mim_val *)malloc(sizeof(mim_val) + cap);
            if (!r) mim_panic("out of memory");
            memcpy(r, xs, sizeof(mim_val) + old);
            heap_state *h = &t_heap;
            padded_count *lc = live_of(h);
            __atomic_store_n(&lc->n, lc->n + 1, __ATOMIC_RELAXED);
            free_block(h, xs);
            xs = r;
        }
        xs->desc = (const mim_desc *)(uintptr_t)cap;
    }
    xs->len = n;
    if (es == 1) ((char *)xs->data)[n] = 0;
    return xs;
}

/* push(xs, x), xs owned */
mim_val *mim_std_push_own(mim_val *xs, mim_val *x) {
    if (own_plain(xs) && x != xs && x && x->rank == 0 && x->dtype == xs->dtype) {
        int64_t n = xs->len;
        xs = grow_owned(xs, n + 1);
        memcpy((char *)xs->data + (size_t)n * elsize(xs->dtype), x->data, elsize(xs->dtype));
        return xs;
    }
    mim_val *r = mim_std_push(xs, x);
    mim_release(xs);
    return r;
}

/* concat(xs, ys), xs owned */
mim_val *mim_std_concat_own(mim_val *xs, mim_val *ys) {
    if (own_plain(xs) && ys != xs && ys && ys->rank == 1 && ys->dtype == xs->dtype && xs->dtype != MIM_CHAR) {
        int64_t n = xs->len, m = ys->len;
        size_t es = elsize(xs->dtype);
        xs = grow_owned(xs, n + m);
        memcpy((char *)xs->data + (size_t)n * es, ys->data, (size_t)m * es);
        return xs;
    }
    mim_val *r = mim_std_concat(xs, ys);
    mim_release(xs);
    return r;
}

/* The same for a list of i64 with raw numbers (compiled code: no boxing).
 * Anything else (not held once, not i64, out of range) takes the usual path. */
mim_val *mim_push_i64_own(mim_val *xs, int64_t v) {
    if (own_plain(xs) && xs->dtype == MIM_I64) {
        int64_t n = xs->len;
        xs = grow_owned(xs, n + 1);
        ((int64_t *)xs->data)[n] = v;
        return xs;
    }
    mim_val *x = mim_int(v);
    mim_val *r = mim_std_push_own(xs, x);
    mim_release(x);
    return r;
}

mim_val *mim_concat_i64_own(mim_val *xs, const int64_t *ys, int64_t m) {
    if (own_plain(xs) && xs->dtype == MIM_I64) {
        int64_t n = xs->len;
        xs = grow_owned(xs, n + m);
        memcpy((int64_t *)xs->data + n, ys, sizeof(int64_t) * (size_t)m);
        return xs;
    }
    mim_val *y = alloc(MIM_I64, 1, m);
    memcpy(y->data, ys, sizeof(int64_t) * (size_t)m);
    mim_val *r = mim_std_concat_own(xs, y);
    mim_release(y);
    return r;
}

mim_val *mim_put_i64_own(mim_val *xs, int64_t i, int64_t v) {
    if (own_plain(xs) && xs->dtype == MIM_I64 && i >= 0 && i < xs->len) {
        ((int64_t *)xs->data)[i] = v;
        return xs;
    }
    mim_val *k = mim_int(i), *x = mim_int(v);
    mim_val *r = mim_std_put_own(xs, k, x);
    mim_release(k);
    mim_release(x);
    return r;
}

/* put(xs, i, v) on a list, xs owned */
mim_val *mim_std_put_own(mim_val *xs, mim_val *k, mim_val *v) {
    if (own_plain(xs) && v != xs && k && k->rank == 0 && (k->dtype == MIM_I64 || k->dtype == MIM_I32) && v && v->rank == 0 && v->dtype == xs->dtype) {
        int64_t i = geti(k, 0);
        if (i >= 0 && i < xs->len) {
            memcpy((char *)xs->data + (size_t)i * elsize(xs->dtype), v->data, elsize(xs->dtype));
            return xs;
        }
    }
    mim_val *r = mim_std_put(xs, k, v);
    mim_release(xs);
    return r;
}

mim_val *mim_std_push(mim_val *xs, mim_val *x) {
    need(xs, "push");
    if (xs->rank == 0) mim_panic("push: expected a list or array");
    if (xs->rank >= 2) mim_panic("push: expected a list, found a matrix (reshape it with ⥊ first)");
    if (xs->dtype == MIM_U8 && x && x->rank == 0 && x->dtype != MIM_F64 && geti(x, 0) >= 0 && geti(x, 0) <= 255) {
        mim_val *r = alloc(MIM_U8, 1, xs->len + 1); /* a byte onto bytes stays bytes */
        memcpy(r->data, xs->data, (size_t)xs->len);
        ((uint8_t *)r->data)[xs->len] = (uint8_t)geti(x, 0);
        return r;
    }
    int64_t n = xs->len;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n + 1));
    for (int64_t i = 0; i < n; i++) tmp[i] = scalar_at(xs, i);
    mim_retain(x);
    tmp[n] = x;
    mim_val *r = list_take(n + 1, tmp);
    free(tmp);
    return r;
}

mim_val *mim_std_concat(mim_val *xs, mim_val *ys) {
    need(xs, "concat");
    need(ys, "concat");
    if (is_str(xs) && is_str(ys)) {
        mim_val *r = alloc(MIM_CHAR, 1, xs->len + ys->len);
        memcpy(r->data, xs->data, (size_t)xs->len);
        memcpy((char *)r->data + xs->len, ys->data, (size_t)ys->len);
        ((char *)r->data)[xs->len + ys->len] = 0;
        return r;
    }
    if (xs->rank >= 2 || ys->rank >= 2) mim_panic("concat: expected lists, found a matrix (reshape it with ⥊ first)");
    /* two arrays of the same plain element type (bytes, numbers): one copy */
    if (xs->rank == 1 && ys->rank == 1 && xs->dtype == ys->dtype && !IS_HANDLES(xs)) {
        size_t es = elsize(xs->dtype);
        mim_val *r = alloc(xs->dtype, 1, xs->len + ys->len);
        memcpy(r->data, xs->data, (size_t)xs->len * es);
        memcpy((char *)r->data + (size_t)xs->len * es, ys->data, (size_t)ys->len * es);
        return r;
    }
    int64_t n = xs->rank == 0 ? 1 : xs->len, m = ys->rank == 0 ? 1 : ys->len;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n + m + 1));
    for (int64_t i = 0; i < n; i++) tmp[i] = xs->rank == 0 ? (mim_retain(xs), xs) : scalar_at(xs, i);
    for (int64_t i = 0; i < m; i++) tmp[n + i] = ys->rank == 0 ? (mim_retain(ys), ys) : scalar_at(ys, i);
    mim_val *r = n + m == 0 ? alloc(MIM_I64, 1, 0) : list_take(n + m, tmp);
    free(tmp);
    return r;
}

static int cmp_vals(const mim_val *a, const mim_val *b) {
    if (is_str(a) && is_str(b)) {
        int64_t n = a->len < b->len ? a->len : b->len;
        int c = memcmp(a->data, b->data, (size_t)n);
        return c ? c : (a->len > b->len) - (a->len < b->len);
    }
    if (a->rank == 0 && b->rank == 0 && IS_NUMDT(a->dtype) && IS_NUMDT(b->dtype)) {
        if (IS_FLOAT(a->dtype) || IS_FLOAT(b->dtype)) {
            double x = getf(a, 0), y = getf(b, 0);
            return (x > y) - (x < y);
        }
        int64_t x = geti(a, 0), y = geti(b, 0);
        return (x > y) - (x < y);
    }
    mim_panic("sort: elements must be numbers or strings");
    return 0;
}

static int cmp_handles(const void *x, const void *y) { return cmp_vals(*(mim_val *const *)x, *(mim_val *const *)y); }

/* ---- sorting numbers: a parallel LSD radix sort ----------------------------
 * Keys are unsigned 64-bit images of the values that sort in the same order
 * (signed integers: flip the sign bit; floats: flip every bit of negatives,
 * the sign bit of the rest). One pass per byte that differs between keys:
 * each chunk counts its bytes, the counts become each chunk's write offsets,
 * and each chunk scatters its keys there — stable, and the same for any
 * number of threads. */

typedef struct {
    uint64_t *src, *dst;
    int64_t grain;
    int shift;
    int64_t *hist; /* 256 per chunk */
} radix_job;

static void radix_count(void *c, int64_t lo, int64_t hi) {
    radix_job *r = (radix_job *)c;
    for (int64_t start = lo; start < hi; start += r->grain) { /* whole chunks, even when run in one call */
        int64_t end = start + r->grain < hi ? start + r->grain : hi;
        int64_t *h = r->hist + (start / r->grain) * 256;
        memset(h, 0, 256 * sizeof(int64_t));
        for (int64_t i = start; i < end; i++) h[(r->src[i] >> r->shift) & 255]++;
    }
}

static void radix_scatter(void *c, int64_t lo, int64_t hi) {
    radix_job *r = (radix_job *)c;
    for (int64_t start = lo; start < hi; start += r->grain) {
        int64_t end = start + r->grain < hi ? start + r->grain : hi;
        int64_t *h = r->hist + (start / r->grain) * 256;
        for (int64_t i = start; i < end; i++) {
            uint64_t v = r->src[i];
            r->dst[h[(v >> r->shift) & 255]++] = v;
        }
    }
}

static void radix_sort_keys(uint64_t *a, int64_t n) {
    if (n < 2) return;
    /* a few keys (a median of 9, per pixel): an insertion sort, with no
     * buffers to allocate and clear */
    if (n <= 64) {
        for (int64_t i = 1; i < n; i++) {
            uint64_t t = a[i];
            int64_t j = i - 1;
            while (j >= 0 && a[j] > t) {
                a[j + 1] = a[j];
                j--;
            }
            a[j + 1] = t;
        }
        return;
    }
    uint64_t diff = 0;
    for (int64_t i = 1; i < n; i++) diff |= a[i] ^ a[0];
    if (!diff) return;
    int64_t grain = n / ((int64_t)g_nthreads * 4);
    if (grain < 65536) grain = 65536;
    int64_t chunks = (n + grain - 1) / grain;
    uint64_t *tmp = (uint64_t *)malloc(sizeof(uint64_t) * (size_t)n);
    int64_t *hist = (int64_t *)malloc(sizeof(int64_t) * 256 * (size_t)chunks);
    if (!tmp || !hist) mim_panic("out of memory");
    radix_job r = {a, tmp, grain, 0, hist};
    for (int shift = 0; shift < 64; shift += 8) {
        if (((diff >> shift) & 255) == 0) continue; /* every key has the same byte here */
        r.shift = shift;
        parallel_for_grain(n, grain, radix_count, &r);
        int64_t at = 0;
        for (int d = 0; d < 256; d++)
            for (int64_t ch = 0; ch < chunks; ch++) {
                int64_t k = hist[ch * 256 + d];
                hist[ch * 256 + d] = at;
                at += k;
            }
        parallel_for_grain(n, grain, radix_scatter, &r);
        uint64_t *t = r.src;
        r.src = r.dst;
        r.dst = t;
    }
    if (r.src != a) memcpy(a, r.src, sizeof(uint64_t) * (size_t)n);
    free(tmp);
    free(hist);
}

/* A few numbers on the caller's stack (compiled `sort(⍳ 9 ¨ f)[4]`),
 * sorted in place: an insertion sort, as sort() orders them (floats by their
 * order as numbers, NaN last as the radix sort puts it) */
void mim_sort_i64_small(int64_t *a, int64_t n) {
    for (int64_t i = 1; i < n; i++) {
        int64_t t = a[i], j = i - 1;
        while (j >= 0 && a[j] > t) {
            a[j + 1] = a[j];
            j--;
        }
        a[j + 1] = t;
    }
}
void mim_sort_f64_small(double *a, int64_t n) {
    /* the same keys as sort_numbers: order-preserving integers */
    uint64_t k[64];
    const uint64_t top = (uint64_t)1 << 63;
    for (int64_t i = 0; i < n; i++) {
        uint64_t b;
        memcpy(&b, &a[i], 8);
        k[i] = (b & top) ? ~b : b | top;
    }
    radix_sort_keys(k, n);
    for (int64_t i = 0; i < n; i++) {
        uint64_t b = (k[i] & top) ? k[i] & ~top : ~k[i];
        memcpy(&a[i], &b, 8);
    }
}

/* a sorted copy of a vector of numbers, bytes or bools; NULL for others */
static mim_val *sort_numbers(mim_val *xs) {
    int64_t n = xs->len;
    if (xs->dtype == MIM_I64 || xs->dtype == MIM_F64) {
        mim_val *r = alloc(xs->dtype, 1, n);
        uint64_t *k = (uint64_t *)r->data;
        memcpy(k, xs->data, sizeof(uint64_t) * (size_t)n);
        const uint64_t top = (uint64_t)1 << 63;
        if (xs->dtype == MIM_I64) {
            for (int64_t i = 0; i < n; i++) k[i] ^= top;
            radix_sort_keys(k, n);
            for (int64_t i = 0; i < n; i++) k[i] ^= top;
        } else {
            for (int64_t i = 0; i < n; i++) k[i] = (k[i] & top) ? ~k[i] : k[i] | top;
            radix_sort_keys(k, n);
            for (int64_t i = 0; i < n; i++) k[i] = (k[i] & top) ? k[i] & ~top : ~k[i];
        }
        return r;
    }
    if (xs->dtype == MIM_U8 || xs->dtype == MIM_BOOL) { /* counting */
        int64_t count[256] = {0};
        const uint8_t *s = (const uint8_t *)xs->data;
        for (int64_t i = 0; i < n; i++) count[s[i]]++;
        mim_val *r = alloc(xs->dtype, 1, n);
        uint8_t *d = (uint8_t *)r->data;
        for (int v = 0; v < 256; v++)
            for (int64_t c = 0; c < count[v]; c++) *d++ = (uint8_t)v;
        return r;
    }
    return NULL;
}

/* sort(xs): ascending; numbers or strings (stable for equal elements) */
mim_val *mim_std_sort(mim_val *xs) {
    need(xs, "sort");
    if (xs->rank >= 2) mim_panic("sort: expected a list, found a matrix");
    if (xs->rank == 0) {
        mim_retain(xs);
        return xs;
    }
    mim_val *fast = sort_numbers(xs);
    if (fast) return fast;
    int64_t n = xs->len;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n + 1));
    for (int64_t i = 0; i < n; i++) tmp[i] = scalar_at(xs, i);
    /* insertion-merge: qsort is not stable, so sort indices by (value, position) */
    for (int64_t w = 1; w < n; w *= 2) {
        mim_val **out = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
        for (int64_t lo = 0; lo < n; lo += 2 * w) {
            int64_t mid = lo + w < n ? lo + w : n, hi = lo + 2 * w < n ? lo + 2 * w : n;
            int64_t i = lo, j = mid, k = lo;
            while (i < mid && j < hi) out[k++] = cmp_handles(&tmp[j], &tmp[i]) < 0 ? tmp[j++] : tmp[i++];
            while (i < mid) out[k++] = tmp[i++];
            while (j < hi) out[k++] = tmp[j++];
        }
        free(tmp);
        tmp = out;
    }
    mim_val *r = n == 0 ? alloc(MIM_I64, 1, 0) : list_take(n, tmp);
    free(tmp);
    return r;
}

/* ==== maps ================================================================= */
/* MIM_MAP: entries in insertion order as handle pairs (k0 v0 k1 v1 …, `len`
 * = 2 × capacity) plus a hash index in a side table kept in `desc`. */

typedef struct {
    int64_t count;  /* live entries */
    int64_t cap;    /* entry capacity */
    int64_t nslots; /* power of two ≥ 2 × cap */
    int64_t slot[]; /* entry index + 1, 0 = empty */
} map_meta;

#define MAP_META(m) ((map_meta *)(void *)(m)->desc)

static uint64_t hash_val(const mim_val *v) {
    uint64_t h = 1469598103934665603ULL;
    if (!v) mim_panic("map key: missing value");
    if (is_str(v)) {
        for (int64_t i = 0; i < v->len; i++) h = (h ^ ((const unsigned char *)v->data)[i]) * 1099511628211ULL;
        return h;
    }
    if (v->rank == 0 && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL || v->dtype == MIM_CHAR || v->dtype == MIM_I32)) {
        uint64_t x = (uint64_t)geti(v, 0) + 0x9E3779B97F4A7C15ULL; /* (an i32 key equals the i64 of its value) */
        x = (x ^ (x >> 30)) * 0xBF58476D1CE4E5B9ULL;
        x = (x ^ (x >> 27)) * 0x94D049BB133111EBULL;
        return x ^ (x >> 31);
    }
    if (v->rank == 0 && IS_FLOAT(v->dtype)) {
        double d = getf(v, 0); /* (an f32 key hashes as its value) */
        uint64_t x;
        memcpy(&x, &d, sizeof x);
        return x * 0x9E3779B97F4A7C15ULL;
    }
    char buf[128];
    snprintf(buf, sizeof buf, "map keys must be numbers, bools, chars or strings, found %s", dtname(v));
    mim_panic(buf);
    return 0;
}

static int vals_equal(const mim_val *a, const mim_val *b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    if (is_str(a) || is_str(b)) return is_str(a) && is_str(b) && a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0;
    if (a->rank == 0 && b->rank == 0 && (IS_NUMDT(a->dtype) || a->dtype == MIM_CHAR) && (IS_NUMDT(b->dtype) || b->dtype == MIM_CHAR)) {
        if (IS_FLOAT(a->dtype) || IS_FLOAT(b->dtype)) return a->dtype == b->dtype && getf(a, 0) == getf(b, 0);
        return geti(a, 0) == geti(b, 0) && (a->dtype == MIM_CHAR) == (b->dtype == MIM_CHAR);
    }
    return 0;
}

/* ==== structural equality ================================================ */

static int map_equal(const mim_val *a, const mim_val *b);

/* Element i of an array as a comparable scalar: 0 = int, 1 = float, 2 = char. */
static int elem_num(const mim_val *v, int64_t i, int64_t *iv, double *fv) {
    if (IS_FLOAT(v->dtype)) { *fv = getf(v, i); return 1; }
    *iv = geti(v, i);
    *fv = (double)*iv;
    return v->dtype == MIM_CHAR ? 2 : 0;
}

/* `a == b` on whole values: numbers by value (1 == 1.0), strings and arrays
 * by length and elements, records/variants/tuples by type and fields, maps by
 * their key → value pairs (in any order), functions by identity. */
int64_t mim_equal(const mim_val *a, const mim_val *b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    if (a->rank != b->rank) return 0;
    if (a->rank >= 2 && a->dtype != MIM_MAP && !same_shape(a, b)) return 0;
    if (a->dtype == MIM_FN || b->dtype == MIM_FN) return 0;
    if (a->dtype == MIM_MAP || b->dtype == MIM_MAP) return a->dtype == b->dtype && map_equal(a, b);
    if (a->dtype == MIM_OBJ && a->rank == 0) {
        if (b->dtype != MIM_OBJ || a->desc != b->desc || a->len != b->len) return 0;
        mim_val **x = (mim_val **)a->data, **y = (mim_val **)b->data;
        for (int64_t i = 0; i < a->len; i++) {
            if (slot_raw(a, i)) { /* the same type: the same layout */
                uint64_t p = ((const uint64_t *)a->data)[i], q = ((const uint64_t *)b->data)[i];
                if ((a->desc->fbits >> i) & 1) {
                    double u, w;
                    memcpy(&u, &p, sizeof u);
                    memcpy(&w, &q, sizeof w);
                    if (u != w) return 0;
                } else if (p != q) {
                    return 0;
                }
                continue;
            }
            if (!mim_equal(x[i], y[i])) return 0;
        }
        return 1;
    }
    if (b->dtype == MIM_OBJ && b->rank == 0) return 0;
    if (a->len != b->len) return 0;
    int ha = IS_HANDLES(a), hb = IS_HANDLES(b);
    if (ha || hb) {
        if (ha && hb) {
            mim_val **x = (mim_val **)a->data, **y = (mim_val **)b->data;
            for (int64_t i = 0; i < a->len; i++)
                if (!mim_equal(x[i], y[i])) return 0;
            return 1;
        }
        /* a list of handles against a flat array: compare element by element */
        for (int64_t i = 0; i < a->len; i++) {
            mim_val *ea = scalar_at(a, i), *eb = scalar_at(b, i);
            int eq = (int)mim_equal(ea, eb);
            mim_release(ea);
            mim_release(eb);
            if (!eq) return 0;
        }
        return 1;
    }
    if ((a->dtype == MIM_CHAR) != (b->dtype == MIM_CHAR)) return 0; /* text is not numbers */
    if (a->dtype == b->dtype && !IS_FLOAT(a->dtype))
        return memcmp(a->data, b->data, (size_t)a->len * elsize(a->dtype)) == 0;
    for (int64_t i = 0; i < a->len; i++) {
        int64_t xi, yi;
        double xf, yf;
        int ka = elem_num(a, i, &xi, &xf), kb = elem_num(b, i, &yi, &yf);
        if (ka == 1 || kb == 1 ? xf != yf : xi != yi) return 0;
    }
    return 1;
}

static mim_val *map_alloc(int64_t cap) {
    if (cap < 4) cap = 4;
    int64_t ns = 8;
    while (ns < 2 * cap) ns *= 2;
    mim_val *m = alloc(MIM_MAP, 0, 2 * cap);
    memset(m->data, 0, sizeof(mim_val *) * (size_t)(2 * cap));
    map_meta *meta = (map_meta *)calloc(1, sizeof(map_meta) + sizeof(int64_t) * (size_t)ns);
    meta->cap = cap;
    meta->nslots = ns;
    m->desc = (const mim_desc *)(void *)meta;
    return m;
}

/* index of the entry holding key k, or -1; *slot_out = where it is or would go */
static int64_t map_find(const mim_val *m, const mim_val *k, int64_t *slot_out) {
    map_meta *meta = MAP_META(m);
    mim_val **h = (mim_val **)m->data;
    int64_t mask = meta->nslots - 1;
    int64_t s = (int64_t)(hash_val(k) & (uint64_t)mask);
    for (;;) {
        int64_t e = meta->slot[s];
        if (e == 0) {
            if (slot_out) *slot_out = s;
            return -1;
        }
        if (vals_equal(h[2 * (e - 1)], k)) {
            if (slot_out) *slot_out = s;
            return e - 1;
        }
        s = (s + 1) & mask;
    }
}

/* insert or replace in a map that has room (takes over owned k and v) */
static void map_put_raw(mim_val *m, mim_val *k, mim_val *v) {
    mim_val **h = (mim_val **)m->data;
    int64_t slot;
    int64_t e = map_find(m, k, &slot);
    if (e >= 0) {
        mim_release(k);
        mim_release(h[2 * e + 1]);
        h[2 * e + 1] = v;
        return;
    }
    map_meta *meta = MAP_META(m);
    e = meta->count++;
    h[2 * e] = k;
    h[2 * e + 1] = v;
    meta->slot[slot] = e + 1;
}

/* a copy with room for `cap` entries */
static mim_val *map_copy(const mim_val *m, int64_t cap) {
    map_meta *meta = MAP_META(m);
    mim_val *r = map_alloc(cap > meta->count ? cap : meta->count);
    mim_val **h = (mim_val **)m->data;
    for (int64_t e = 0; e < meta->count; e++) {
        mim_retain(h[2 * e]);
        mim_retain(h[2 * e + 1]);
        map_put_raw(r, h[2 * e], h[2 * e + 1]);
    }
    return r;
}

static void map_dealloc_meta(mim_val *m) { free(MAP_META(m)); }

static mim_val *map_check(mim_val *m, const char *fn) {
    if (!m || m->dtype != MIM_MAP) {
        char buf[160];
        snprintf(buf, sizeof buf, "%s: expected a map, found %s", fn, dtname(m));
        mim_panic(buf);
    }
    return m;
}

/* [k: v, …] */
mim_val *mim_map_lit(int64_t n, mim_val **keys, mim_val **vals) {
    mim_val *m = map_alloc(n);
    for (int64_t i = 0; i < n; i++) {
        mim_retain(keys[i]);
        mim_retain(vals[i]);
        map_put_raw(m, keys[i], vals[i]);
    }
    return m;
}

static int map_equal(const mim_val *a, const mim_val *b) {
    if (MAP_META(a)->count != MAP_META(b)->count) return 0;
    mim_val **h = (mim_val **)a->data, **g = (mim_val **)b->data;
    for (int64_t e = 0; e < MAP_META(a)->count; e++) {
        int64_t f = map_find(b, h[2 * e], NULL);
        if (f < 0 || !mim_equal(h[2 * e + 1], g[2 * f + 1])) return 0;
    }
    return 1;
}

static mim_val *map_get(mim_val *m, mim_val *k) {
    int64_t e = map_find(m, k, NULL);
    if (e < 0) {
        sbuf b = {0};
        sb_str(&b, "key not found: ");
        fmt_val(&b, k, 1);
        mim_panic(b.p);
    }
    mim_val *v = ((mim_val **)m->data)[2 * e + 1];
    mim_retain(v);
    return v;
}

/* m[k] = v on a `let mut` map: in place when unique, else a copy */
static void map_set(mim_val **slot, mim_val *k, mim_val *v) {
    mim_val *m = *slot;
    map_meta *meta = MAP_META(m);
    int full = map_find(m, k, NULL) < 0 && meta->count == meta->cap;
    if (__atomic_load_n(&m->rc, __ATOMIC_ACQUIRE) != 1 || full) {
        mim_val *c = map_copy(m, full ? meta->cap * 2 : meta->cap);
        mim_release(m);
        *slot = m = c;
    }
    mim_retain(k);
    mim_retain(v);
    map_put_raw(m, k, v);
}

mim_val *mim_std_has(mim_val *m, mim_val *k) { return mim_bool(map_find(map_check(m, "has"), k, NULL) >= 0); }

/* put(m, k, v): a new map with k set; put(xs, i, v): a new list with
 * element i replaced (the original is unchanged either way) */
mim_val *mim_std_put(mim_val *m, mim_val *k, mim_val *v) {
    if (m && m->dtype != MIM_MAP && m->rank == 1) {
        mim_val *slot = m;
        mim_retain(m); /* shared now, so the store copies */
        mim_set_index(&slot, k, v);
        return slot;
    }
    map_check(m, "put");
    mim_val *r = map_copy(m, MAP_META(m)->count + 1);
    mim_retain(k);
    mim_retain(v);
    map_put_raw(r, k, v);
    return r;
}

/* map_of(keys, values): a map from two lists of the same length (a later
 * duplicate key wins); one pass, no copies */
mim_val *mim_std_map_of(mim_val *ks, mim_val *vs) {
    if (!ks || !vs || ks->rank != 1 || vs->rank != 1) mim_panic("map_of: expected two lists");
    if (ks->len != vs->len) {
        char buf[128];
        snprintf(buf, sizeof buf, "map_of: %lld keys but %lld values", (long long)ks->len, (long long)vs->len);
        mim_panic(buf);
    }
    mim_val *m = map_alloc(ks->len);
    for (int64_t i = 0; i < ks->len; i++) map_put_raw(m, scalar_at(ks, i), scalar_at(vs, i));
    return m;
}

/* remove(m, k): a new map without k */
mim_val *mim_std_remove(mim_val *m, mim_val *k) {
    map_check(m, "remove");
    map_meta *meta = MAP_META(m);
    mim_val *r = map_alloc(meta->count);
    mim_val **h = (mim_val **)m->data;
    for (int64_t e = 0; e < meta->count; e++) {
        if (vals_equal(h[2 * e], k)) continue;
        mim_retain(h[2 * e]);
        mim_retain(h[2 * e + 1]);
        map_put_raw(r, h[2 * e], h[2 * e + 1]);
    }
    return r;
}

static mim_val *map_column(mim_val *m, int which, const char *fn) {
    map_check(m, fn);
    int64_t n = MAP_META(m)->count;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val **h = (mim_val **)m->data;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    for (int64_t e = 0; e < n; e++) tmp[e] = h[2 * e + which];
    mim_val *r = mim_array_lit(n, tmp);
    free(tmp);
    return r;
}

mim_val *mim_std_keys(mim_val *m) { return map_column(m, 0, "keys"); }
mim_val *mim_std_values(mim_val *m) { return map_column(m, 1, "values"); }

mim_val *mim_std_map_new(void) { return map_alloc(4); }

/* ==== math (when the compiler does not know the numbers are floats) ======= */

static double math_arg(mim_val *x, const char *fn) {
    if (!x || x->rank != 0 || !(IS_NUMDT(x->dtype) && x->dtype != MIM_BOOL)) {
        char buf[96];
        snprintf(buf, sizeof buf, "math::%s: expected a number", fn);
        mim_panic(buf);
    }
    return getf(x, 0);
}

#define MATH1(name, expr) \
    mim_val *mim_std_math_##name(mim_val *a) { double x = math_arg(a, #name); return mim_float(expr); }
#define MATH2(name, expr) \
    mim_val *mim_std_math_##name(mim_val *a, mim_val *b) { double x = math_arg(a, #name), y = math_arg(b, #name); return mim_float(expr); }

MATH1(sqrt, sqrt(x))
MATH1(sin, sin(x))
MATH1(cos, cos(x))
MATH1(tan, tan(x))
MATH1(asin, asin(x))
MATH1(acos, acos(x))
MATH1(atan, atan(x))
MATH2(atan2, atan2(x, y))
MATH2(hypot, hypot(x, y))
MATH1(exp, exp(x))
MATH1(ln, log(x))
MATH1(log2, log2(x))
MATH1(log10, log10(x))
MATH2(pow, pow(x, y))
MATH1(floor, floor(x))
MATH1(ceil, ceil(x))
MATH1(round, round(x))

mim_val *mim_std_math_to_int(mim_val *a) {
    double x = math_arg(a, "to_int");
    if (!(x >= -9223372036854775808.0 && x < 9223372036854775808.0)) mim_panic("math::to_int: the number is NaN or too large for an i64");
    return mim_int((int64_t)x);
}


/* ==== tests (`mimic test`) ================================================= */

/* failed expectations in this run (the test is run in a process of its own) */
static int64_t g_test_failures = 0;

/* property tests run their cases quietly while they search */
static int g_expect_quiet = 0;

mim_val *mim_std_expect_quiet(mim_val *on) {
    g_expect_quiet = mim_truthy(on);
    return NULL;
}

mim_val *mim_std_expect_failures(void) { return mim_int(__atomic_load_n(&g_test_failures, __ATOMIC_RELAXED)); }

mim_val *mim_std_expect_forgive(mim_val *count) {
    __atomic_store_n(&g_test_failures, geti(count, 0), __ATOMIC_RELAXED);
    return NULL;
}

/* a property failed: its smallest case (the failure counts) */
mim_val *mim_std_prop_failed(mim_val *text, mim_val *line) {
    __atomic_fetch_add(&g_test_failures, 1, __ATOMIC_RELAXED);
    fflush(stdout);
    fprintf(stderr, "  line %lld: the property fails; the smallest failing case: %.*s\n", (long long)geti(line, 0), text ? (int)text->len : 0, text ? (const char *)text->data : "");
    return NULL;
}

static void expect_report(mim_val *text, int64_t line) {
    __atomic_fetch_add(&g_test_failures, 1, __ATOMIC_RELAXED);
    if (g_expect_quiet) return;
    fflush(stdout);
    fprintf(stderr, "  line %lld: expect(%.*s)\n", (long long)line, text ? (int)text->len : 0, text ? (const char *)text->data : "");
}

static void expect_show(const char *label, mim_val *v) {
    if (g_expect_quiet) return;
    sbuf b = {0};
    fmt_val(&b, v, 1);
    fprintf(stderr, "    %s %.*s\n", label, (int)b.n, b.p ? b.p : "");
    free(b.p);
}

/* expect(cond): the source text and line are added by the compiler */
mim_val *mim_std_expect_true(mim_val *ok, mim_val *text, mim_val *line) {
    if (!mim_truthy(ok)) expect_report(text, geti(line, 0));
    return NULL;
}

/* expect(a OP b) for a comparison: both sides are shown when it fails */
mim_val *mim_std_expect_cmp(mim_val *op, mim_val *a, mim_val *b, mim_val *text, mim_val *line) {
    int o = (int)geti(op, 0), ok;
    if (o == OP_EQ || o == OP_NE) {
        ok = (int)mim_equal(a, b) == (o == OP_EQ);
    } else {
        mim_val *r = mim_binop(o, a, b);
        ok = 1;
        for (int64_t i = 0; i < (r->rank == 0 ? 1 : r->len); i++) ok &= geti(r, i) != 0; /* every element */
        mim_release(r);
    }
    if (!ok) {
        expect_report(text, geti(line, 0));
        expect_show("left: ", a);
        expect_show("right:", b);
    }
    return NULL;
}

/* main of a test build: `--list` names the tests, `--run k` runs test k
 * (exit 1 when an expectation failed), anything else runs them all here */
int32_t mim_test_main(int32_t n, const char *const *names, mim_val *(*const *fns)(void), int32_t leak_check) {
    if (g_argc >= 2 && strcmp(g_argv[1], "--list") == 0) {
        for (int32_t i = 0; i < n; i++) printf("%s\n", names[i]);
        return mim_rt_shutdown(0);
    }
    if (g_argc >= 3 && strcmp(g_argv[1], "--run") == 0) {
        int32_t k = atoi(g_argv[2]);
        if (k < 0 || k >= n) mim_panic("no such test");
        mim_release(fns[k]());
        int32_t code = mim_rt_shutdown(leak_check);
        return g_test_failures ? 1 : code;
    }
    int32_t failed = 0;
    for (int32_t i = 0; i < n; i++) {
        int64_t before = g_test_failures;
        mim_release(fns[i]());
        int bad = g_test_failures != before;
        failed += bad;
        printf("%s %s\n", bad ? "✗" : "✓", names[i]);
    }
    printf("%d passed, %d failed\n", n - failed, failed);
    int32_t code = mim_rt_shutdown(leak_check);
    return failed ? 1 : code;
}

/* ==== characters, padding, words, slices, bases (0.2.6) =================== */

static void sb_putc(sbuf *b, char c) { sb_put(b, &c, 1); }

/* the number of characters (UTF-8 code points) in a byte range */
static int64_t utf8_count(const char *p, int64_t n) {
    int64_t k = 0;
    for (int64_t i = 0; i < n; i++)
        if (((unsigned char)p[i] & 0xC0) != 0x80) k++;
    return k;
}

/* char_code("A") → 65: the first character's code point */
mim_val *mim_std_char_code(mim_val *s) {
    const unsigned char *p = (const unsigned char *)str_arg(s, "char_code");
    int64_t n = s->len;
    if (n == 0) mim_panic("char_code: the text is empty");
    uint32_t c = p[0];
    int more = c < 0x80 ? 0 : c < 0xE0 ? 1 : c < 0xF0 ? 2 : 3;
    if (more) c &= 0x3F >> more;
    for (int i = 1; i <= more && i < n; i++) c = c << 6 | (p[i] & 0x3F);
    return mim_int((int64_t)c);
}

/* char_of(65) → "A" */
mim_val *mim_std_char_of(mim_val *code) {
    int64_t c = int_arg(code, "char_of");
    if (c < 0 || c > 0x10FFFF || (c >= 0xD800 && c <= 0xDFFF)) {
        char buf[96];
        snprintf(buf, sizeof buf, "char_of: %lld is not a Unicode character", (long long)c);
        mim_panic(buf);
    }
    char b[4];
    int n;
    if (c < 0x80) { b[0] = (char)c; n = 1; }
    else if (c < 0x800) { b[0] = (char)(0xC0 | c >> 6); b[1] = (char)(0x80 | (c & 0x3F)); n = 2; }
    else if (c < 0x10000) { b[0] = (char)(0xE0 | c >> 12); b[1] = (char)(0x80 | (c >> 6 & 0x3F)); b[2] = (char)(0x80 | (c & 0x3F)); n = 3; }
    else { b[0] = (char)(0xF0 | c >> 18); b[1] = (char)(0x80 | (c >> 12 & 0x3F)); b[2] = (char)(0x80 | (c >> 6 & 0x3F)); b[3] = (char)(0x80 | (c & 0x3F)); n = 4; }
    return mim_str(b, n);
}

/* text padded to `width` characters with copies of `fill` (its first character) */
static mim_val *pad_text(mim_val *s, mim_val *width, mim_val *fill, int at_start, const char *fn) {
    const char *a = str_arg(s, fn), *f = str_arg(fill, fn);
    int64_t w = int_arg(width, fn), have = utf8_count(a, s->len);
    if (fill->len == 0) mim_panic("pad: the fill text is empty");
    int64_t flen = 1;
    while (flen < fill->len && ((unsigned char)f[flen] & 0xC0) == 0x80) flen++;
    int64_t pad = w > have ? w - have : 0;
    mim_val *r = alloc(MIM_CHAR, 1, s->len + pad * flen);
    char *o = (char *)r->data;
    if (!at_start) { memcpy(o, a, (size_t)s->len); o += s->len; }
    for (int64_t i = 0; i < pad; i++) { memcpy(o, f, (size_t)flen); o += flen; }
    if (at_start) { memcpy(o, a, (size_t)s->len); o += s->len; }
    *o = 0;
    return r;
}

mim_val *mim_std_text_pad_start(mim_val *s, mim_val *width, mim_val *fill) { return pad_text(s, width, fill, 1, "pad_start"); }
mim_val *mim_std_text_pad_end(mim_val *s, mim_val *width, mim_val *fill) { return pad_text(s, width, fill, 0, "pad_end"); }

/* words("a  b\tc") → ["a" "b" "c"]: split on runs of whitespace */
mim_val *mim_std_words(mim_val *s) {
    const char *a = str_arg(s, "words");
    int64_t n = s->len, k = 0, cap = 8;
    mim_val **parts = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
    for (int64_t i = 0; i < n;) {
        while (i < n && strchr(" \t\n\r\f\v", a[i])) i++;
        int64_t j = i;
        while (j < n && !strchr(" \t\n\r\f\v", a[j])) j++;
        if (j > i) {
            if (k == cap) parts = (mim_val **)realloc(parts, sizeof(mim_val *) * (size_t)(cap *= 2));
            parts[k++] = mim_str(a + i, j - i);
        }
        i = j;
    }
    mim_val *r = k == 0 ? alloc(MIM_BOXED, 1, 0) : list_take(k, parts);
    free(parts);
    return r;
}

/* slice(text, from, to): the bytes from..to (to excluded), as `find` and `≢`
 * count them; negative positions count from the end; a position inside a
 * multi-byte character moves to that character's edge (never half of one) */
mim_val *mim_std_slice(mim_val *s, mim_val *from, mim_val *to) {
    const char *a = str_arg(s, "slice");
    int64_t n = s->len, i = int_arg(from, "slice"), j = int_arg(to, "slice");
    if (i < 0) i += n;
    if (j < 0) j += n;
    i = i < 0 ? 0 : i > n ? n : i;
    j = j < 0 ? 0 : j > n ? n : j;
    while (i > 0 && i < n && ((unsigned char)a[i] & 0xC0) == 0x80) i--;
    while (j < n && j > 0 && ((unsigned char)a[j] & 0xC0) == 0x80) j++;
    return mim_str(a + i, j > i ? j - i : 0);
}

/* an integer in base 2..36 ("ff", "-1010", "0xff" for 16): 1 and the value,
 * or 0 when the text is not one (or it overflows) */
static int int_in_base(const char *a, int64_t n, int64_t base, int64_t *out) {
    if (base < 2 || base > 36) {
        char buf[64];
        snprintf(buf, sizeof buf, "parse_int: base %lld is not between 2 and 36", (long long)base);
        mim_panic(buf);
    }
    int64_t i = 0;
    while (i < n && (a[i] == ' ' || a[i] == '\t')) i++;
    while (n > i && (a[n - 1] == ' ' || a[n - 1] == '\t' || a[n - 1] == '\n' || a[n - 1] == '\r')) n--;
    int neg = 0;
    if (i < n && (a[i] == '-' || a[i] == '+')) neg = a[i++] == '-';
    if (i + 1 < n && a[i] == '0') {
        char p = (char)(a[i + 1] | 0x20);
        if ((p == 'x' && base == 16) || (p == 'b' && base == 2) || (p == 'o' && base == 8)) i += 2;
    }
    if (i >= n) return 0;
    uint64_t v = 0, lim = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    for (; i < n; i++) {
        char c = a[i];
        if (c == '_') continue;
        int d = c >= '0' && c <= '9' ? c - '0' : (c | 0x20) >= 'a' && (c | 0x20) <= 'z' ? (c | 0x20) - 'a' + 10 : 99;
        if (d >= base) return 0;
        if (v > (lim - (uint64_t)d) / (uint64_t)base) return 0;
        v = v * (uint64_t)base + (uint64_t)d;
    }
    *out = neg ? (int64_t)(0 - v) : (int64_t)v;
    return 1;
}

mim_val *mim_std_is_int_base(mim_val *s, mim_val *base) {
    int64_t v;
    return mim_bool(int_in_base(str_arg(s, "parse_int"), s->len, int_arg(base, "parse_int"), &v));
}

mim_val *mim_std_to_int_base(mim_val *s, mim_val *base) {
    int64_t v = 0;
    int_in_base(str_arg(s, "parse_int"), s->len, int_arg(base, "parse_int"), &v);
    return mim_int(v);
}

/* ==== format(value, spec): `"{x:.2}"` (0.2.6) ===============================
 * spec = [[fill]align][sign][#][0][width][,|_][.precision][type]
 *   align  < left, > right, ^ centre (numbers default right, text left)
 *   sign   + always, - only negative (default), space: a space for positive
 *   #      0x, 0o, 0b before hex, octal, binary
 *   0      pad numbers with zeros after the sign
 *   ,  _   thousands separators (groups of 4 digits with _ in x, o, b)
 *   type   x X o b (integers in base 16, 8, 2), e E (exponent), f (fixed),
 *          % (percent), s (text), d (decimal) */

typedef struct {
    char fill[5];
    int fill_n;
    char align, sign, type;
    int alt, zero, group;
    int64_t width, prec;
} fspec;

/* parse a spec; 0 with a message when it is not one */
static int fspec_parse(const char *s, int64_t n, fspec *f, char *why, size_t whyn) {
    memset(f, 0, sizeof *f);
    f->fill[0] = ' ';
    f->fill_n = 1;
    f->prec = -1;
    int64_t i = 0;
    int64_t c1 = 1; /* bytes of the first character */
    while (c1 < n && ((unsigned char)s[c1] & 0xC0) == 0x80) c1++;
    if (c1 < n && strchr("<>^", s[c1])) {
        memcpy(f->fill, s, (size_t)c1);
        f->fill_n = (int)c1;
        f->align = s[c1];
        i = c1 + 1;
    } else if (i < n && strchr("<>^", s[i])) {
        f->align = s[i++];
    }
    if (i < n && strchr("+- ", s[i])) f->sign = s[i++];
    if (i < n && s[i] == '#') { f->alt = 1; i++; }
    if (i < n && s[i] == '0') { f->zero = 1; i++; }
    while (i < n && s[i] >= '0' && s[i] <= '9') f->width = f->width * 10 + (s[i++] - '0');
    if (i < n && (s[i] == ',' || s[i] == '_')) f->group = s[i++];
    if (i < n && s[i] == '.') {
        i++;
        if (i >= n || s[i] < '0' || s[i] > '9') { snprintf(why, whyn, "a precision after `.`, as in `.2`"); return 0; }
        f->prec = 0;
        while (i < n && s[i] >= '0' && s[i] <= '9') f->prec = f->prec * 10 + (s[i++] - '0');
    }
    if (i < n && strchr("xXobeEf%sd", s[i])) f->type = s[i++];
    if (i != n) { snprintf(why, whyn, "`%.*s` is not part of a format (as in `>8`, `05`, `.2`, `x`, `+,.3f`)", (int)(n - i), s + i); return 0; }
    return 1;
}

/* digits with a separator every `every` from the right */
static void group_digits(sbuf *b, const char *d, int64_t n, char sep, int every) {
    for (int64_t i = 0; i < n; i++) {
        if (i > 0 && (n - i) % every == 0) sb_putc(b, sep);
        sb_putc(b, d[i]);
    }
}

mim_val *mim_std_format(mim_val *v, mim_val *specv) {
    const char *spec = str_arg(specv, "format");
    fspec f;
    char why[160];
    if (!fspec_parse(spec, specv->len, &f, why, sizeof why)) {
        char buf[260];
        snprintf(buf, sizeof buf, "format: %s", why);
        mim_panic(buf);
    }
    sbuf body = {0};   /* the digits or text */
    const char *pre = ""; /* sign and 0x, before zero padding */
    char prebuf[8];
    int numeric = v && v->rank == 0 && IS_NUMDT(v->dtype) && v->dtype != MIM_BOOL;
    if (numeric && !IS_FLOAT(v->dtype) && !strchr("eEf%", f.type ? f.type : 'd')) {
        int64_t x = geti(v, 0);
        uint64_t u = x < 0 ? (uint64_t)0 - (uint64_t)x : (uint64_t)x;
        int base = f.type == 'x' || f.type == 'X' ? 16 : f.type == 'o' ? 8 : f.type == 'b' ? 2 : 10;
        char d[72];
        int k = 0;
        do {
            int r = (int)(u % (uint64_t)base);
            d[k++] = (char)(r < 10 ? '0' + r : (f.type == 'X' ? 'A' : 'a') + r - 10);
            u /= (uint64_t)base;
        } while (u);
        for (int a = 0, z = k - 1; a < z; a++, z--) { char t = d[a]; d[a] = d[z]; d[z] = t; }
        if (f.group) group_digits(&body, d, k, f.group, base == 10 ? 3 : 4);
        else sb_put(&body, d, k);
        int p = 0;
        if (x < 0) prebuf[p++] = '-';
        else if (f.sign == '+' || f.sign == ' ') prebuf[p++] = f.sign;
        if (f.alt && base != 10) { prebuf[p++] = '0'; prebuf[p++] = f.type == 'X' ? 'X' : (char)f.type; }
        prebuf[p] = 0;
        pre = prebuf;
    } else if (numeric) {
        double x = IS_FLOAT(v->dtype) ? getf(v, 0) : (double)geti(v, 0);
        char t = f.type;
        if (t == '%') x *= 100;
        int neg = signbit(x) && !isnan(x);
        double ax = fabs(x);
        char d[400];
        if (t == 'e' || t == 'E') snprintf(d, sizeof d, t == 'e' ? "%.*e" : "%.*E", (int)(f.prec < 0 ? 6 : f.prec), ax);
        else if (t == 'f' || t == '%' || f.prec >= 0) snprintf(d, sizeof d, "%.*f", (int)(f.prec < 0 ? 6 : f.prec), ax);
        else { /* as print shows it */
            sbuf tmp = {0};
            mim_val *a = mim_float(ax);
            fmt_val(&tmp, a, 0);
            mim_release(a);
            snprintf(d, sizeof d, "%.*s", (int)tmp.n, tmp.p ? tmp.p : "");
            free(tmp.p);
        }
        if (f.group && isfinite(ax)) {
            int64_t ip = 0, len = (int64_t)strlen(d);
            while (ip < len && d[ip] >= '0' && d[ip] <= '9') ip++;
            group_digits(&body, d, ip, f.group, 3);
            sb_put(&body, d + ip, len - ip);
        } else {
            sb_str(&body, d);
        }
        if (t == '%') sb_putc(&body, '%');
        int p = 0;
        if (neg) prebuf[p++] = '-';
        else if (f.sign == '+' || f.sign == ' ') prebuf[p++] = f.sign;
        prebuf[p] = 0;
        pre = prebuf;
    } else {
        fmt_val(&body, v, 0);
        if (f.prec >= 0) { /* at most `prec` characters */
            int64_t keep = 0, chars = 0;
            while (keep < (int64_t)body.n && chars <= f.prec) {
                if (((unsigned char)body.p[keep] & 0xC0) != 0x80) chars++;
                if (chars > f.prec) break;
                keep++;
            }
            body.n = (size_t)keep;
        }
    }
    int64_t len = utf8_count(pre, (int64_t)strlen(pre)) + utf8_count(body.p ? body.p : "", (int64_t)body.n);
    int64_t pad = f.width > len ? f.width - len : 0;
    sbuf out = {0};
    if (f.zero && numeric && !f.align) {
        sb_str(&out, pre);
        for (int64_t i = 0; i < pad; i++) sb_putc(&out, '0');
        sb_put(&out, body.p, (int64_t)body.n);
    } else {
        char align = f.align ? f.align : numeric ? '>' : '<';
        int64_t left = align == '>' ? pad : align == '^' ? pad / 2 : 0, right = pad - left;
        for (int64_t i = 0; i < left; i++) sb_put(&out, f.fill, f.fill_n);
        sb_str(&out, pre);
        sb_put(&out, body.p, (int64_t)body.n);
        for (int64_t i = 0; i < right; i++) sb_put(&out, f.fill, f.fill_n);
    }
    mim_val *r = mim_str(out.p ? out.p : "", (int64_t)out.n);
    free(out.p);
    free(body.p);
    return r;
}

/* ==== collections (0.2.7) ================================================== */

/* sort_index(keys): the positions of `keys` in ascending order, stable (equal
 * keys keep their order); for sort_by, min_by, max_by */
mim_val *mim_std_sort_index(mim_val *keys) {
    need(keys, "sort_by");
    if (keys->rank != 1) mim_panic("sort_by: the keys must form a list");
    int64_t n = keys->len;
    mim_val **k = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n + 1));
    int64_t *idx = (int64_t *)malloc(sizeof(int64_t) * (size_t)(n + 1)), *out = (int64_t *)malloc(sizeof(int64_t) * (size_t)(n + 1));
    for (int64_t i = 0; i < n; i++) {
        k[i] = scalar_at(keys, i);
        idx[i] = i;
    }
    for (int64_t w = 1; w < n; w *= 2) { /* bottom-up merge sort: stable */
        for (int64_t lo = 0; lo < n; lo += 2 * w) {
            int64_t mid = lo + w < n ? lo + w : n, hi = lo + 2 * w < n ? lo + 2 * w : n;
            int64_t i = lo, j = mid, o = lo;
            while (i < mid && j < hi) out[o++] = cmp_vals(k[idx[j]], k[idx[i]]) < 0 ? idx[j++] : idx[i++];
            while (i < mid) out[o++] = idx[i++];
            while (j < hi) out[o++] = idx[j++];
        }
        int64_t *t = idx;
        idx = out;
        out = t;
    }
    mim_val *r = alloc(MIM_I64, 1, n);
    memcpy(r->data, idx, sizeof(int64_t) * (size_t)n);
    for (int64_t i = 0; i < n; i++) mim_release(k[i]);
    free(k);
    free(idx);
    free(out);
    return r;
}

/* group(keys, xs): a map from each key (in order of first appearance) to the
 * elements of xs with that key; for group_by */
mim_val *mim_std_group(mim_val *keys, mim_val *xs) {
    need(keys, "group_by");
    need(xs, "group_by");
    if (keys->rank != 1 || xs->rank != 1 || keys->len != xs->len) mim_panic("group_by: expected a list");
    int64_t n = keys->len, groups = 0;
    mim_val *ids = map_alloc(n);
    int64_t *gid = (int64_t *)malloc(sizeof(int64_t) * (size_t)(n + 1));
    int64_t *count = (int64_t *)calloc((size_t)(n + 1), sizeof(int64_t));
    mim_val **gkey = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(n + 1));
    for (int64_t i = 0; i < n; i++) {
        mim_val *k = scalar_at(keys, i);
        int64_t e = map_find(ids, k, NULL);
        if (e < 0) {
            gkey[groups] = k;
            mim_retain(k);
            map_put_raw(ids, k, mim_int(groups));
            gid[i] = groups++;
        } else {
            gid[i] = geti(((mim_val **)ids->data)[2 * e + 1], 0);
            mim_release(k);
        }
        count[gid[i]]++;
    }
    mim_val ***elems = (mim_val ***)malloc(sizeof(mim_val **) * (size_t)(groups + 1));
    int64_t *fill = (int64_t *)calloc((size_t)(groups + 1), sizeof(int64_t));
    for (int64_t g = 0; g < groups; g++) elems[g] = (mim_val **)malloc(sizeof(mim_val *) * (size_t)count[g]);
    for (int64_t i = 0; i < n; i++) elems[gid[i]][fill[gid[i]]++] = scalar_at(xs, i);
    mim_val *r = map_alloc(groups);
    for (int64_t g = 0; g < groups; g++) {
        map_put_raw(r, gkey[g], list_take(count[g], elems[g]));
        free(elems[g]);
    }
    mim_release(ids);
    free(elems);
    free(fill);
    free(gid);
    free(count);
    free(gkey);
    return r;
}

/* contract_failed(message): a broken `requires` or `ensures` (status 101) */
mim_val *mim_std_contract_failed(mim_val *message) {
    fflush(stdout);
    char buf[1200];
    snprintf(buf, sizeof buf, "contract broken: %s", str_arg(message, "contract_failed"));
    mim_panic(buf);
    return NULL;
}

/* panic(message): stop the program with a message (status 101) */
mim_val *mim_std_panic(mim_val *message) {
    fflush(stdout);
    mim_panic(str_arg(message, "panic"));
    return NULL;
}
