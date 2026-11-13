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
    int boxed = 0, dt = -1;
    for (int64_t k = 0; k < n; k++) {
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
    if (a->rank == 0 && b->rank == 0 && a->dtype <= MIM_BOOL && b->dtype <= MIM_BOOL) {
        if (a->dtype == MIM_F64 || b->dtype == MIM_F64) {
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
    if (v->rank == 0 && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL || v->dtype == MIM_CHAR)) {
        uint64_t x = (uint64_t)geti(v, 0) + 0x9E3779B97F4A7C15ULL;
        x = (x ^ (x >> 30)) * 0xBF58476D1CE4E5B9ULL;
        x = (x ^ (x >> 27)) * 0x94D049BB133111EBULL;
        return x ^ (x >> 31);
    }
    if (v->rank == 0 && v->dtype == MIM_F64) {
        double d = getf(v, 0);
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
    if (a->rank == 0 && b->rank == 0 && a->dtype <= MIM_CHAR && b->dtype <= MIM_CHAR) {
        if (a->dtype == MIM_F64 || b->dtype == MIM_F64) return a->dtype == b->dtype && getf(a, 0) == getf(b, 0);
        return geti(a, 0) == geti(b, 0) && (a->dtype == MIM_CHAR) == (b->dtype == MIM_CHAR);
    }
    return 0;
}

/* ==== structural equality ================================================ */

static int map_equal(const mim_val *a, const mim_val *b);

/* Element i of an array as a comparable scalar: 0 = int, 1 = float, 2 = char. */
static int elem_num(const mim_val *v, int64_t i, int64_t *iv, double *fv) {
    if (v->dtype == MIM_F64) { *fv = getf(v, i); return 1; }
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
        for (int64_t i = 0; i < a->len; i++)
            if (!mim_equal(x[i], y[i])) return 0;
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
    if (a->dtype == b->dtype && a->dtype != MIM_F64)
        return memcmp(a->data, b->data, (size_t)a->len * (size_t)(a->dtype == MIM_I64 ? 8 : 1)) == 0;
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
    if (!x || x->rank != 0 || (x->dtype != MIM_F64 && x->dtype != MIM_I64 && x->dtype != MIM_U8)) {
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

