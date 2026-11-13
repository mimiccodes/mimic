/* The json module: JSON text ↔ Mimic values. Pure functions.
 * Included at the end of mimic_rt.c (same translation unit), after mimic_io.c.
 *
 *   json::encode(value) / json::encode_pretty(value)   any value → JSON text
 *   json::parse(text) → Result[json::Value, str]        an untyped tree
 *   json::decode[T](text) → Result[T, str]              follows T's schema
 *
 * `decode` receives a schema the compiler emits for T (see Cg::schema):
 *   kind 0 any, 1 int, 2 float, 3 bool, 4 str, 5 list, 7 record, 8 Option,
 *   9 tuple, 10 other union (not decodable yet). */

typedef struct mim_schema {
    int32_t kind, n;
    const mim_desc *desc;
    const struct mim_schema *const *items;
} mim_schema;

extern const mim_desc mim_desc_json_Null, mim_desc_json_Bool, mim_desc_json_Number, mim_desc_json_Text, mim_desc_json_List, mim_desc_json_Object;

/* ---- parsing into a C tree ------------------------------------------------ */

enum { J_NULL, J_FALSE, J_TRUE, J_NUM, J_STR, J_ARR, J_OBJ };

typedef struct jnode {
    int type;
    double num;
    int64_t inum;
    int is_int;
    char *str; /* J_STR (NUL-terminated, may contain NULs: use len) */
    size_t len;
    int64_t n, cap;
    struct jnode **kids;
    char **keys;
    size_t *key_lens;
} jnode;

typedef struct {
    const char *s;
    size_t i, n;
    char err[256];
} jparser;

static void jfree(jnode *j) {
    if (!j) return;
    for (int64_t k = 0; k < j->n; k++) {
        jfree(j->kids[k]);
        if (j->keys) free(j->keys[k]);
    }
    free(j->kids);
    free(j->keys);
    free(j->key_lens);
    free(j->str);
    free(j);
}

static void jfail(jparser *p, const char *what) {
    if (p->err[0]) return;
    int64_t line = 1, col = 1;
    for (size_t k = 0; k < p->i && k < p->n; k++) {
        if (p->s[k] == '\n') {
            line++;
            col = 1;
        } else {
            col++;
        }
    }
    snprintf(p->err, sizeof p->err, "invalid JSON at line %lld, column %lld: %s", (long long)line, (long long)col, what);
}

static void jws(jparser *p) {
    while (p->i < p->n && (p->s[p->i] == ' ' || p->s[p->i] == '\t' || p->s[p->i] == '\n' || p->s[p->i] == '\r')) p->i++;
}

static void jpush(jnode *j, jnode *kid, char *key, size_t klen) {
    if (j->n == j->cap) {
        j->cap = j->cap ? j->cap * 2 : 4;
        j->kids = (jnode **)realloc(j->kids, sizeof(jnode *) * (size_t)j->cap);
        if (j->type == J_OBJ) {
            j->keys = (char **)realloc(j->keys, sizeof(char *) * (size_t)j->cap);
            j->key_lens = (size_t *)realloc(j->key_lens, sizeof(size_t) * (size_t)j->cap);
        }
    }
    j->kids[j->n] = kid;
    if (j->type == J_OBJ) {
        j->keys[j->n] = key;
        j->key_lens[j->n] = klen;
    }
    j->n++;
}

static int hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static void put_utf8(sbuf *b, uint32_t cp) {
    char u[4];
    size_t n;
    if (cp < 0x80) { u[0] = (char)cp; n = 1; }
    else if (cp < 0x800) { u[0] = (char)(0xC0 | (cp >> 6)); u[1] = (char)(0x80 | (cp & 0x3F)); n = 2; }
    else if (cp < 0x10000) { u[0] = (char)(0xE0 | (cp >> 12)); u[1] = (char)(0x80 | ((cp >> 6) & 0x3F)); u[2] = (char)(0x80 | (cp & 0x3F)); n = 3; }
    else { u[0] = (char)(0xF0 | (cp >> 18)); u[1] = (char)(0x80 | ((cp >> 12) & 0x3F)); u[2] = (char)(0x80 | ((cp >> 6) & 0x3F)); u[3] = (char)(0x80 | (cp & 0x3F)); n = 4; }
    sb_put(b, u, n);
}

/* a string literal (p->s[p->i] == '"'); the text in *out (malloc'd) */
static int jstring(jparser *p, char **out, size_t *len) {
    p->i++;
    sbuf b = {0};
    while (p->i < p->n) {
        char c = p->s[p->i++];
        if (c == '"') {
            sb_put(&b, "\0", 1); /* a NUL after the text */
            *len = b.n - 1;
            *out = b.p;
            return 1;
        }
        if ((unsigned char)c < 0x20) {
            free(b.p);
            jfail(p, "control character in a string");
            return 0;
        }
        if (c != '\\') {
            sb_put(&b, &c, 1);
            continue;
        }
        if (p->i >= p->n) break;
        char e = p->s[p->i++];
        switch (e) {
        case '"': sb_put(&b, "\"", 1); break;
        case '\\': sb_put(&b, "\\", 1); break;
        case '/': sb_put(&b, "/", 1); break;
        case 'b': sb_put(&b, "\b", 1); break;
        case 'f': sb_put(&b, "\f", 1); break;
        case 'n': sb_put(&b, "\n", 1); break;
        case 'r': sb_put(&b, "\r", 1); break;
        case 't': sb_put(&b, "\t", 1); break;
        case 'u': {
            uint32_t cp = 0;
            for (int k = 0; k < 4; k++) {
                int h = p->i < p->n ? hexval(p->s[p->i++]) : -1;
                if (h < 0) {
                    free(b.p);
                    jfail(p, "bad \\u escape");
                    return 0;
                }
                cp = cp * 16 + (uint32_t)h;
            }
            /* a surrogate pair: 😀 */
            if (cp >= 0xD800 && cp <= 0xDBFF && p->i + 6 <= p->n && p->s[p->i] == '\\' && p->s[p->i + 1] == 'u') {
                uint32_t lo = 0;
                int ok = 1;
                for (int k = 0; k < 4; k++) {
                    int h = hexval(p->s[p->i + 2 + k]);
                    if (h < 0) ok = 0;
                    lo = lo * 16 + (uint32_t)(h < 0 ? 0 : h);
                }
                if (ok && lo >= 0xDC00 && lo <= 0xDFFF) {
                    p->i += 6;
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                }
            }
            put_utf8(&b, cp);
            break;
        }
        default:
            free(b.p);
            jfail(p, "unknown escape in a string");
            return 0;
        }
    }
    free(b.p);
    jfail(p, "unterminated string");
    return 0;
}

/* A number (p->s[p->i] is '-' or a digit). Integers of up to 18 digits and
 * decimals that are exact in a double (up to 15 digits, a small exponent:
 * one correctly rounded division or multiplication) skip strtod. */
static void jnum(jparser *p, int *is_int, int64_t *inum, double *num) {
    static const double p10[] = {1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11,
                                 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22};
    size_t st = p->i;
    int neg = 0, simple = 1;
    if (p->s[p->i] == '-') {
        neg = 1;
        p->i++;
    }
    uint64_t m = 0;
    int digits = 0, frac = 0;
    *is_int = 1;
    while (p->i < p->n && p->s[p->i] >= '0' && p->s[p->i] <= '9') {
        if (digits < 18) m = m * 10 + (uint64_t)(p->s[p->i] - '0');
        else simple = 0;
        if (m || digits) digits++;
        p->i++;
    }
    if (p->i < p->n && p->s[p->i] == '.') {
        *is_int = 0;
        p->i++;
        while (p->i < p->n && p->s[p->i] >= '0' && p->s[p->i] <= '9') {
            if (digits < 18) {
                m = m * 10 + (uint64_t)(p->s[p->i] - '0');
                frac++;
                if (m) digits++;
            } else {
                simple = 0;
            }
            p->i++;
        }
    }
    int ex = 0;
    if (p->i < p->n && (p->s[p->i] == 'e' || p->s[p->i] == 'E')) {
        *is_int = 0;
        p->i++;
        int eneg = 0;
        if (p->i < p->n && (p->s[p->i] == '+' || p->s[p->i] == '-')) eneg = p->s[p->i++] == '-';
        while (p->i < p->n && p->s[p->i] >= '0' && p->s[p->i] <= '9') {
            if (ex < 10000) ex = ex * 10 + (p->s[p->i] - '0');
            p->i++;
        }
        if (eneg) ex = -ex;
    }
    int e10 = ex - frac;
    if (simple && *is_int) {
        *inum = neg ? -(int64_t)m : (int64_t)m;
        *num = neg && m == 0 ? -0.0 : (double)*inum; /* exact below 2^53; else strtod's below */
        if (m < (1ULL << 53)) return;
    } else if (simple && m < (1ULL << 53) && e10 >= -22 && e10 <= 22) {
        double x = (double)m;
        x = e10 < 0 ? x / p10[-e10] : x * p10[e10];
        *num = neg ? -x : x;
        return;
    }
    char tmp[64];
    size_t len = p->i - st < sizeof tmp - 1 ? p->i - st : sizeof tmp - 1;
    memcpy(tmp, p->s + st, len);
    tmp[len] = 0;
    *num = strtod(tmp, NULL);
    if (*is_int) {
        errno = 0;
        long long v = strtoll(tmp, NULL, 10);
        *is_int = errno == 0;
        *inum = v;
    }
}

static jnode *jvalue(jparser *p, int depth) {
    if (depth > 512) {
        jfail(p, "nested too deeply");
        return NULL;
    }
    jws(p);
    if (p->i >= p->n) {
        jfail(p, "unexpected end of input");
        return NULL;
    }
    jnode *j = (jnode *)calloc(1, sizeof(jnode));
    char c = p->s[p->i];
    if (c == '{' || c == '[') {
        j->type = c == '{' ? J_OBJ : J_ARR;
        char close = c == '{' ? '}' : ']';
        p->i++;
        jws(p);
        if (p->i < p->n && p->s[p->i] == close) {
            p->i++;
            return j;
        }
        for (;;) {
            char *key = NULL;
            size_t klen = 0;
            if (j->type == J_OBJ) {
                jws(p);
                if (p->i >= p->n || p->s[p->i] != '"') {
                    jfail(p, "expected a key (a string)");
                    jfree(j);
                    return NULL;
                }
                if (!jstring(p, &key, &klen)) {
                    jfree(j);
                    return NULL;
                }
                jws(p);
                if (p->i >= p->n || p->s[p->i] != ':') {
                    free(key);
                    jfail(p, "expected ':'");
                    jfree(j);
                    return NULL;
                }
                p->i++;
            }
            jnode *kid = jvalue(p, depth + 1);
            if (!kid) {
                free(key);
                jfree(j);
                return NULL;
            }
            jpush(j, kid, key, klen);
            jws(p);
            if (p->i < p->n && p->s[p->i] == ',') {
                p->i++;
                continue;
            }
            if (p->i < p->n && p->s[p->i] == close) {
                p->i++;
                return j;
            }
            jfail(p, j->type == J_OBJ ? "expected ',' or '}'" : "expected ',' or ']'");
            jfree(j);
            return NULL;
        }
    }
    if (c == '"') {
        j->type = J_STR;
        if (!jstring(p, &j->str, &j->len)) {
            free(j);
            return NULL;
        }
        return j;
    }
    if (p->n - p->i >= 4 && memcmp(p->s + p->i, "true", 4) == 0) { p->i += 4; j->type = J_TRUE; return j; }
    if (p->n - p->i >= 5 && memcmp(p->s + p->i, "false", 5) == 0) { p->i += 5; j->type = J_FALSE; return j; }
    if (p->n - p->i >= 4 && memcmp(p->s + p->i, "null", 4) == 0) { p->i += 4; j->type = J_NULL; return j; }
    if (c == '-' || (c >= '0' && c <= '9')) {
        j->type = J_NUM;
        jnum(p, &j->is_int, &j->inum, &j->num);
        return j;
    }
    free(j);
    jfail(p, "expected a value");
    return NULL;
}

/* the whole text as one value (nothing but whitespace after it) */
static jnode *jparse_all(mim_val *text, char *err, size_t errn) {
    if (!text || text->dtype != MIM_CHAR || text->rank != 1) mim_panic("json: expected text (str)");
    jparser p = {(const char *)text->data, 0, (size_t)text->len, {0}};
    jnode *j = jvalue(&p, 0);
    if (j) {
        jws(&p);
        if (p.i < p.n) {
            jfail(&p, "unexpected text after the value");
            jfree(j);
            j = NULL;
        }
    }
    if (!j) snprintf(err, errn, "%s", p.err);
    return j;
}

/* ---- tree → json::Value --------------------------------------------------- */

static mim_val *variant1(const mim_desc *d, mim_val *v) { return obj_owned(d, 1, &v); }

static mim_val *map_from_pairs(int64_t n, mim_val **keys, mim_val **vals) {
    mim_val *m = mim_map_lit(n, keys, vals);
    for (int64_t k = 0; k < n; k++) {
        mim_release(keys[k]);
        mim_release(vals[k]);
    }
    return m;
}

static mim_val *list_owned(int64_t n, mim_val **xs) {
    if (n == 0) return alloc(MIM_BOXED, 1, 0);
    mim_val *r = mim_array_lit(n, xs);
    for (int64_t k = 0; k < n; k++) mim_release(xs[k]);
    return r;
}

static mim_val *to_value(const jnode *j) {
    switch (j->type) {
    case J_NULL: return obj_owned(&mim_desc_json_Null, 0, NULL);
    case J_TRUE: return variant1(&mim_desc_json_Bool, mim_bool(1));
    case J_FALSE: return variant1(&mim_desc_json_Bool, mim_bool(0));
    case J_NUM: return variant1(&mim_desc_json_Number, mim_float(j->num));
    case J_STR: return variant1(&mim_desc_json_Text, mim_str(j->str, (int64_t)j->len));
    case J_ARR: {
        mim_val **xs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        for (int64_t k = 0; k < j->n; k++) xs[k] = to_value(j->kids[k]);
        /* a list of values stays a list of handles even when empty */
        mim_val *l = j->n ? list_owned(j->n, xs) : alloc(MIM_BOXED, 1, 0);
        free(xs);
        return variant1(&mim_desc_json_List, l);
    }
    default: {
        mim_val **ks = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        mim_val **vs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        for (int64_t k = 0; k < j->n; k++) {
            ks[k] = mim_str(j->keys[k], (int64_t)j->key_lens[k]);
            vs[k] = to_value(j->kids[k]);
        }
        mim_val *m = map_from_pairs(j->n, ks, vs);
        free(ks);
        free(vs);
        return variant1(&mim_desc_json_Object, m);
    }
    }
}

/* ---- tree → plain values (for `any`: maps, lists, numbers, …) ------------- */

static mim_val *to_plain(const jnode *j) {
    switch (j->type) {
    case J_NULL: return none_val();
    case J_TRUE: return mim_bool(1);
    case J_FALSE: return mim_bool(0);
    case J_NUM: return j->is_int ? mim_int(j->inum) : mim_float(j->num);
    case J_STR: return mim_str(j->str, (int64_t)j->len);
    case J_ARR: {
        mim_val **xs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        for (int64_t k = 0; k < j->n; k++) xs[k] = to_plain(j->kids[k]);
        mim_val *l = list_owned(j->n, xs);
        free(xs);
        return l;
    }
    default: {
        mim_val **ks = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        mim_val **vs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        for (int64_t k = 0; k < j->n; k++) {
            ks[k] = mim_str(j->keys[k], (int64_t)j->key_lens[k]);
            vs[k] = to_plain(j->kids[k]);
        }
        mim_val *m = map_from_pairs(j->n, ks, vs);
        free(ks);
        free(vs);
        return m;
    }
    }
}

/* ---- tree → typed values (decode) ----------------------------------------- */

static const char *jtype_name(const jnode *j) {
    switch (j->type) {
    case J_NULL: return "null";
    case J_TRUE:
    case J_FALSE: return "a bool";
    case J_NUM: return "a number";
    case J_STR: return "a string";
    case J_ARR: return "an array";
    default: return "an object";
    }
}

/* NULL on error, with the message (and the path to the value) in err */
static mim_val *to_typed(const jnode *j, const mim_schema *s, const char *where, char *err, size_t errn) {
    char here[256];
#define WANT(what)                                                                                    \
    do {                                                                                              \
        snprintf(err, errn, "json: expected %s at %s, found %s", what, where[0] ? where : "the top", jtype_name(j)); \
        return NULL;                                                                                  \
    } while (0)
    switch (s->kind) {
    case 0: return to_plain(j);
    case 1:
        if (j->type != J_NUM) WANT("an integer");
        if (!j->is_int) {
            if (j->num != (double)(int64_t)j->num) WANT("an integer");
            return mim_int((int64_t)j->num);
        }
        return mim_int(j->inum);
    case 2:
        if (j->type != J_NUM) WANT("a number");
        return mim_float(j->num);
    case 3:
        if (j->type != J_TRUE && j->type != J_FALSE) WANT("true or false");
        return mim_bool(j->type == J_TRUE);
    case 4:
        if (j->type != J_STR) WANT("a string");
        return mim_str(j->str, (int64_t)j->len);
    case 5: {
        if (j->type != J_ARR) WANT("an array");
        mim_val **xs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(j->n ? j->n : 1));
        for (int64_t k = 0; k < j->n; k++) {
            snprintf(here, sizeof here, "%s[%lld]", where, (long long)k);
            xs[k] = to_typed(j->kids[k], s->items[0], here, err, errn);
            if (!xs[k]) {
                for (int64_t q = 0; q < k; q++) mim_release(xs[q]);
                free(xs);
                return NULL;
            }
        }
        mim_val *l = j->n ? list_owned(j->n, xs) : alloc(s->items[0]->kind == 1 ? MIM_I64 : s->items[0]->kind == 2 ? MIM_F64 : MIM_BOXED, 1, 0);
        free(xs);
        return l;
    }
    case 7: {
        if (j->type != J_OBJ) WANT("an object");
        const mim_desc *d = s->desc;
        mim_val **f = (mim_val **)calloc((size_t)(s->n ? s->n : 1), sizeof(mim_val *));
        for (int32_t q = 0; q < s->n; q++) {
            const char *name = d->fields[q];
            size_t nl = strlen(name);
            snprintf(here, sizeof here, "%s%s%s", where, where[0] ? "." : "", name);
            const jnode *v = NULL;
            for (int64_t k = 0; k < j->n; k++)
                if (j->key_lens[k] == nl && memcmp(j->keys[k], name, nl) == 0) v = j->kids[k];
            if (!v) {
                if (s->items[q]->kind == 8) { /* a missing optional field is None */
                    f[q] = none_val();
                    continue;
                }
                snprintf(err, errn, "json: missing field `%s`", here);
            } else {
                f[q] = to_typed(v, s->items[q], here, err, errn);
            }
            if (!f[q]) {
                for (int32_t r = 0; r < q; r++) mim_release(f[r]);
                free(f);
                return NULL;
            }
        }
        mim_val *r = obj_owned(d, s->n, f);
        free(f);
        return r;
    }
    case 8: {
        if (j->type == J_NULL) return none_val();
        mim_val *v = to_typed(j, s->items[0], where, err, errn);
        return v ? some_of(v) : NULL;
    }
    case 9: {
        if (j->type != J_ARR || j->n != s->n) {
            char what[64];
            snprintf(what, sizeof what, "an array of %d", s->n);
            WANT(what);
        }
        mim_val **f = (mim_val **)calloc((size_t)(s->n ? s->n : 1), sizeof(mim_val *));
        for (int32_t q = 0; q < s->n; q++) {
            snprintf(here, sizeof here, "%s[%d]", where, q);
            f[q] = to_typed(j->kids[q], s->items[q], here, err, errn);
            if (!f[q]) {
                for (int32_t r = 0; r < q; r++) mim_release(f[r]);
                free(f);
                return NULL;
            }
        }
        mim_val *r = obj_owned(s->desc, s->n, f); /* the schema carries the tuple descriptor */
        free(f);
        return r;
    }
    default:
        snprintf(err, errn, "json: decoding into this union type is not supported yet (records, lists, Option, tuples and scalars are)");
        return NULL;
    }
#undef WANT
}

/* ---- text → typed values in one pass (decode) ---------------------------- *
 * Follows the schema while it reads, building no tree. Any problem gives
 * NULL: decode then runs the tree path above, which explains it. */

/* A string's text without escapes: points into the input. */
static int jraw_string(jparser *p, const char **s, size_t *n) {
    size_t k = p->i + 1;
    while (k < p->n) {
        unsigned char c = (unsigned char)p->s[k];
        if (c == '"') {
            *s = p->s + p->i + 1;
            *n = k - p->i - 1;
            p->i = k + 1;
            return 1;
        }
        if (c == '\\' || c < 0x20) return 0;
        k++;
    }
    return 0;
}

static mim_val *jdec_str(jparser *p) {
    const char *s;
    size_t n;
    if (jraw_string(p, &s, &n)) return mim_str(s, (int64_t)n);
    char *t;
    if (!jstring(p, &t, &n)) return NULL;
    mim_val *v = mim_str(t, (int64_t)n);
    free(t);
    return v;
}

static int jlit(jparser *p, const char *w, size_t n) {
    if (p->n - p->i >= n && memcmp(p->s + p->i, w, n) == 0) {
        p->i += n;
        return 1;
    }
    return 0;
}

/* A record or tuple of decoded fields, taking them over. */
static mim_val *obj_take(const mim_desc *d, int64_t n, mim_val **f) {
    mim_val *v = alloc(MIM_OBJ, 0, n);
    v->desc = d;
    if (n) memcpy(v->data, f, sizeof(mim_val *) * (size_t)n);
    return v;
}

static mim_val *jdec(jparser *p, const mim_schema *s, int depth);

/* `[ … ]` into a C array of values; -1 on failure (released) */
static int64_t jdec_items(jparser *p, const mim_schema *item, int depth, mim_val ***out) {
    p->i++;
    int64_t n = 0, cap = 16;
    mim_val **xs = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap);
    jws(p);
    if (p->i < p->n && p->s[p->i] == ']') {
        p->i++;
        *out = xs;
        return 0;
    }
    for (;;) {
        mim_val *v = jdec(p, item, depth + 1);
        if (!v) break;
        if (n == cap) {
            cap *= 2;
            xs = (mim_val **)realloc(xs, sizeof(mim_val *) * (size_t)cap);
        }
        xs[n++] = v;
        jws(p);
        if (p->i < p->n && p->s[p->i] == ',') {
            p->i++;
            continue;
        }
        if (p->i < p->n && p->s[p->i] == ']') {
            p->i++;
            *out = xs;
            return n;
        }
        break;
    }
    for (int64_t k = 0; k < n; k++) mim_release(xs[k]);
    free(xs);
    return -1;
}

static mim_val *jdec(jparser *p, const mim_schema *s, int depth) {
    if (depth > 512) return NULL;
    jws(p);
    if (p->i >= p->n) return NULL;
    char c = p->s[p->i];
    switch (s->kind) {
    case 0: {
        jnode *j = jvalue(p, depth);
        if (!j) return NULL;
        mim_val *v = to_plain(j);
        jfree(j);
        return v;
    }
    case 1:
    case 2: {
        if (c != '-' && (c < '0' || c > '9')) return NULL;
        int is_int;
        int64_t inum = 0;
        double num;
        jnum(p, &is_int, &inum, &num);
        if (s->kind == 2) return mim_float(num);
        if (is_int) return mim_int(inum);
        if (num != (double)(int64_t)num) return NULL;
        return mim_int((int64_t)num);
    }
    case 3:
        if (jlit(p, "true", 4)) return mim_bool(1);
        if (jlit(p, "false", 5)) return mim_bool(0);
        return NULL;
    case 4: return c == '"' ? jdec_str(p) : NULL;
    case 5: {
        if (c != '[') return NULL;
        mim_val **xs;
        int64_t n = jdec_items(p, s->items[0], depth, &xs);
        if (n < 0) return NULL;
        mim_val *l = n ? list_take(n, xs) : alloc(s->items[0]->kind == 1 ? MIM_I64 : s->items[0]->kind == 2 ? MIM_F64 : MIM_BOXED, 1, 0);
        free(xs);
        return l;
    }
    case 7: {
        if (c != '{') return NULL;
        p->i++;
        const mim_desc *d = s->desc;
        mim_val *small[16];
        mim_val **f = s->n <= 16 ? small : (mim_val **)malloc(sizeof(mim_val *) * (size_t)s->n);
        for (int32_t q = 0; q < s->n; q++) f[q] = NULL;
        int ok = 0;
        jws(p);
        if (p->i < p->n && p->s[p->i] == '}') {
            p->i++;
            ok = 1;
        } else {
            for (;;) {
                jws(p);
                if (p->i >= p->n || p->s[p->i] != '"') break;
                const char *key;
                size_t kl;
                char *owned = NULL;
                if (!jraw_string(p, &key, &kl)) {
                    if (!jstring(p, &owned, &kl)) break;
                    key = owned;
                }
                int32_t q = 0;
                while (q < s->n && !(strlen(d->fields[q]) == kl && memcmp(d->fields[q], key, kl) == 0)) q++;
                free(owned);
                jws(p);
                if (p->i >= p->n || p->s[p->i] != ':') break;
                p->i++;
                if (q < s->n) {
                    mim_val *v = jdec(p, s->items[q], depth + 1);
                    if (!v) break;
                    if (f[q]) mim_release(f[q]); /* a repeated key: the last one counts */
                    f[q] = v;
                } else {
                    jnode *skip = jvalue(p, depth + 1); /* not a field: ignored */
                    if (!skip) break;
                    jfree(skip);
                }
                jws(p);
                if (p->i < p->n && p->s[p->i] == ',') {
                    p->i++;
                    continue;
                }
                if (p->i < p->n && p->s[p->i] == '}') {
                    p->i++;
                    ok = 1;
                }
                break;
            }
        }
        for (int32_t q = 0; ok && q < s->n; q++) {
            if (f[q]) continue;
            if (s->items[q]->kind == 8) f[q] = none_val(); /* a missing optional field */
            else ok = 0;
        }
        mim_val *r = NULL;
        if (ok) {
            r = obj_take(d, s->n, f);
        } else {
            for (int32_t q = 0; q < s->n; q++)
                if (f[q]) mim_release(f[q]);
        }
        if (f != small) free(f);
        return r;
    }
    case 8: {
        if (jlit(p, "null", 4)) return none_val();
        mim_val *v = jdec(p, s->items[0], depth);
        return v ? some_of(v) : NULL;
    }
    case 9: {
        if (c != '[') return NULL;
        mim_val **xs = NULL;
        p->i++;
        int64_t n = 0;
        int ok = 0;
        xs = (mim_val **)calloc((size_t)(s->n ? s->n : 1), sizeof(mim_val *));
        jws(p);
        if (s->n == 0) {
            if (p->i < p->n && p->s[p->i] == ']') {
                p->i++;
                ok = 1;
            }
        } else {
            for (;;) {
                if (n == s->n) break;
                mim_val *v = jdec(p, s->items[n], depth + 1);
                if (!v) break;
                xs[n++] = v;
                jws(p);
                if (p->i < p->n && p->s[p->i] == ',') {
                    p->i++;
                    continue;
                }
                if (p->i < p->n && p->s[p->i] == ']' && n == s->n) {
                    p->i++;
                    ok = 1;
                }
                break;
            }
        }
        mim_val *r = NULL;
        if (ok) r = obj_take(s->desc, s->n, xs);
        else
            for (int64_t k = 0; k < n; k++) mim_release(xs[k]);
        free(xs);
        return r;
    }
    default: return NULL;
    }
}

mim_val *mim_std_json_parse(mim_val *text) {
    char err[256];
    jnode *j = jparse_all(text, err, sizeof err);
    if (!j) return err_of(err);
    mim_val *v = to_value(j);
    jfree(j);
    return ok_of(v);
}

mim_val *mim_std_json_decode(mim_val *schema, mim_val *text) {
    char err[320];
    if (text && text->dtype == MIM_CHAR && text->rank == 1) {
        jparser fp = {(const char *)text->data, 0, (size_t)text->len, {0}};
        mim_val *v = jdec(&fp, (const mim_schema *)(const void *)schema, 0);
        if (v) {
            jws(&fp);
            if (fp.i == fp.n) return ok_of(v);
            mim_release(v);
        }
    }
    /* something is wrong: the tree path says what */
    jnode *j = jparse_all(text, err, sizeof err);
    if (!j) return err_of(err);
    mim_val *v = to_typed(j, (const mim_schema *)(const void *)schema, "", err, sizeof err);
    jfree(j);
    return v ? ok_of(v) : err_of(err);
}

/* ---- encoding ------------------------------------------------------------- */

static void jenc_str(sbuf *b, const char *s, size_t n) {
    sb_put(b, "\"", 1);
    size_t run = 0; /* bytes copied as they are, in one piece */
    for (size_t k = 0; k < n; k++) {
        unsigned char c = (unsigned char)s[k];
        if (c >= 0x20 && c != '"' && c != '\\') continue;
        sb_put(b, s + run, k - run);
        run = k + 1;
        switch (c) {
        case '"': sb_put(b, "\\\"", 2); break;
        case '\\': sb_put(b, "\\\\", 2); break;
        case '\n': sb_put(b, "\\n", 2); break;
        case '\r': sb_put(b, "\\r", 2); break;
        case '\t': sb_put(b, "\\t", 2); break;
        default: {
            char u[8];
            snprintf(u, sizeof u, "\\u%04x", c);
            sb_put(b, u, 6);
        }
        }
    }
    sb_put(b, s + run, n - run);
    sb_put(b, "\"", 1);
}

/* x (finite, |x| in [1e-4, 1e15) or 0) as a decimal of at most 6 places
 * that reads back exactly, with the fewest places: what %.15g/%.17g would
 * write. 0 when there is none. */
static size_t fmt_short_decimal(char *out, double x) {
    static const double p10[] = {1, 10, 100, 1000, 10000, 100000, 1000000};
    double ax = x < 0 ? -x : x;
    if (ax >= 1e15 || (ax != 0 && ax < 1e-4)) return 0;
    for (int k = 0; k <= 6; k++) {
        double m = ax * p10[k];
        if (m >= 9007199254740992.0) return 0;
        int64_t mi = (int64_t)(m + 0.5);
        if ((double)mi / p10[k] != ax) continue;
        char dig[24];
        size_t nd = fmt_i64(dig, mi), o = 0;
        if (signbit(x)) out[o++] = '-';
        if (k == 0) {
            memcpy(out + o, dig, nd);
            o += nd;
            memcpy(out + o, ".0", 2);
            return o + 2;
        }
        if (nd <= (size_t)k) { /* 0.0…digits */
            out[o++] = '0';
            out[o++] = '.';
            for (size_t z = nd; z < (size_t)k; z++) out[o++] = '0';
            memcpy(out + o, dig, nd);
            return o + nd;
        }
        memcpy(out + o, dig, nd - (size_t)k);
        o += nd - (size_t)k;
        out[o++] = '.';
        memcpy(out + o, dig + nd - (size_t)k, (size_t)k);
        return o + (size_t)k;
    }
    return 0;
}

static void jenc_num(sbuf *b, double x) {
    if (x != x || x == 1.0 / 0.0 || x == -1.0 / 0.0) {
        sb_str(b, "null");
        return;
    }
    char tmp[40];
    size_t fast = fmt_short_decimal(tmp, x);
    if (fast) {
        sb_put(b, tmp, fast);
        return;
    }
    for (int prec = 15; prec <= 17; prec++) { /* the shortest text that reads back exactly */
        snprintf(tmp, sizeof tmp, "%.*g", prec, x);
        if (strtod(tmp, NULL) == x) break;
    }
    sb_str(b, tmp);
    if (!strpbrk(tmp, ".eEn")) sb_str(b, ".0"); /* stays a float */
}

static void jnl(sbuf *b, int indent, int depth) {
    if (indent <= 0) return;
    sb_put(b, "\n", 1);
    for (int k = 0; k < indent * depth; k++) sb_put(b, " ", 1);
}

static void jenc(sbuf *b, const mim_val *v, int indent, int depth);

static void jenc_elems(sbuf *b, const mim_val *v, int64_t from, int64_t n, int indent, int depth) {
    sb_put(b, "[", 1);
    for (int64_t k = 0; k < n; k++) {
        if (k) sb_put(b, ",", 1);
        jnl(b, indent, depth + 1);
        int64_t i = from + k;
        switch (v->dtype) { /* elements in place: no boxing */
        case MIM_BOXED: jenc(b, ((mim_val *const *)v->data)[i], indent, depth + 1); continue;
        case MIM_I64: sb_i64(b, geti(v, i)); continue;
        case MIM_F64: jenc_num(b, getf(v, i)); continue;
        case MIM_BOOL: sb_str(b, geti(v, i) ? "true" : "false"); continue;
        default: break;
        }
        mim_val *e = scalar_at(v, i);
        jenc(b, e, indent, depth + 1);
        mim_release(e);
    }
    if (n) jnl(b, indent, depth);
    sb_put(b, "]", 1);
}

/* an array of rank ≥ 2: nested JSON arrays along the leading axis */
static void jenc_shaped(sbuf *b, const mim_val *v, int axis, int64_t from, int indent, int depth) {
    int64_t d = dim_of(v, axis), cell = 1;
    for (int k = axis + 1; k < v->rank; k++) cell *= dim_of(v, k);
    if (axis == v->rank - 1) {
        jenc_elems(b, v, from, d, indent, depth);
        return;
    }
    sb_put(b, "[", 1);
    for (int64_t k = 0; k < d; k++) {
        if (k) sb_put(b, ",", 1);
        jnl(b, indent, depth + 1);
        jenc_shaped(b, v, axis + 1, from + k * cell, indent, depth + 1);
    }
    if (d) jnl(b, indent, depth);
    sb_put(b, "]", 1);
}

static void jenc(sbuf *b, const mim_val *v, int indent, int depth) {
    if (!v) {
        sb_str(b, "null");
        return;
    }
    if (depth > 512) mim_panic("json::encode: nested too deeply (a cycle?)");
    if (v->dtype == MIM_CHAR && v->rank == 1) {
        jenc_str(b, (const char *)v->data, (size_t)v->len);
        return;
    }
    switch (v->dtype) {
    case MIM_FN: mim_panic("json::encode: functions cannot be encoded"); return;
    case MIM_MAP: {
        mim_val **h = (mim_val **)v->data;
        int64_t count = MAP_META(v)->count;
        sb_put(b, "{", 1);
        for (int64_t k = 0; k < count; k++) {
            if (k) sb_put(b, ",", 1);
            jnl(b, indent, depth + 1);
            mim_val *key = h[2 * k];
            if (key->dtype == MIM_CHAR && key->rank == 1) {
                jenc_str(b, (const char *)key->data, (size_t)key->len);
            } else {
                sbuf kb = {0};
                fmt_val(&kb, key, 0);
                jenc_str(b, kb.p ? kb.p : "", kb.n);
                free(kb.p);
            }
            sb_put(b, indent > 0 ? ": " : ":", indent > 0 ? 2 : 1);
            jenc(b, h[2 * k + 1], indent, depth + 1);
        }
        if (count) jnl(b, indent, depth);
        sb_put(b, "}", 1);
        return;
    }
    case MIM_OBJ:
        if (v->rank == 0) {
            const mim_desc *d = v->desc;
            mim_val **f = (mim_val **)v->data;
            if (d == &mim_desc_Some) { jenc(b, f[0], indent, depth); return; }
            if (d == &mim_desc_None || d == &mim_desc_json_Null) { sb_str(b, "null"); return; }
            if (d == &mim_desc_json_Bool || d == &mim_desc_json_Number || d == &mim_desc_json_Text || d == &mim_desc_json_List || d == &mim_desc_json_Object) {
                jenc(b, f[0], indent, depth);
                return;
            }
            if (!d->variant) { /* a record: an object with its field names */
                sb_put(b, "{", 1);
                for (int64_t k = 0; k < v->len; k++) {
                    if (k) sb_put(b, ",", 1);
                    jnl(b, indent, depth + 1);
                    jenc_str(b, d->fields[k], strlen(d->fields[k]));
                    sb_put(b, indent > 0 ? ": " : ":", indent > 0 ? 2 : 1);
                    jenc(b, f[k], indent, depth + 1);
                }
                if (v->len) jnl(b, indent, depth);
                sb_put(b, "}", 1);
                return;
            }
            if (!d->variant[0]) { /* a tuple: an array */
                sb_put(b, "[", 1);
                for (int64_t k = 0; k < v->len; k++) {
                    if (k) sb_put(b, ",", 1);
                    jnl(b, indent, depth + 1);
                    jenc(b, f[k], indent, depth + 1);
                }
                if (v->len) jnl(b, indent, depth);
                sb_put(b, "]", 1);
                return;
            }
            /* another variant: "Name", {"Name": x} or {"Name": [x, y]} */
            if (v->len == 0) {
                jenc_str(b, d->variant, strlen(d->variant));
                return;
            }
            const char *name = d == &mim_desc_Ok ? "ok" : d == &mim_desc_Err ? "error" : d->variant;
            sb_put(b, "{", 1);
            jnl(b, indent, depth + 1);
            jenc_str(b, name, strlen(name));
            sb_put(b, indent > 0 ? ": " : ":", indent > 0 ? 2 : 1);
            if (v->len == 1) {
                jenc(b, f[0], indent, depth + 1);
            } else {
                sb_put(b, "[", 1);
                for (int64_t k = 0; k < v->len; k++) {
                    if (k) sb_put(b, ",", 1);
                    jnl(b, indent, depth + 2);
                    jenc(b, f[k], indent, depth + 2);
                }
                jnl(b, indent, depth + 1);
                sb_put(b, "]", 1);
            }
            jnl(b, indent, depth);
            sb_put(b, "}", 1);
            return;
        }
        break;
    default: break;
    }
    if (v->rank == 0) {
        switch (v->dtype) {
        case MIM_I64: {
            sb_i64(b, geti(v, 0));
            return;
        }
        case MIM_F64: jenc_num(b, getf(v, 0)); return;
        case MIM_BOOL: sb_str(b, geti(v, 0) ? "true" : "false"); return;
        case MIM_CHAR: {
            char c = (char)geti(v, 0);
            jenc_str(b, &c, 1);
            return;
        }
        default: sb_str(b, "null"); return;
        }
    }
    if (v->rank >= 2) {
        jenc_shaped(b, v, 0, 0, indent, depth);
        return;
    }
    jenc_elems(b, v, 0, v->len, indent, depth);
}

mim_val *mim_std_json_encode(mim_val *v) {
    sbuf b = {0};
    jenc(&b, v, 0, 0);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

mim_val *mim_std_json_encode_pretty(mim_val *v) {
    sbuf b = {0};
    jenc(&b, v, 2, 0);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}
