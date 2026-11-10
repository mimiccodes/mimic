/* Mimic runtime — array layer (Milestone 1). See mimic_rt.h for the ownership contract. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef enum { MIM_I64 = 0, MIM_F64 = 1, MIM_BOOL = 2, MIM_CHAR = 3 } mim_dtype;

typedef struct mim_val {
    int64_t rc;
    int32_t dtype;
    int32_t rank;
    int64_t len;
    void *data;
} mim_val;

typedef mim_val *(*mim_fn1)(mim_val *);

enum { OP_ADD, OP_SUB, OP_MUL, OP_DIV, OP_MOD, OP_EQ, OP_NE, OP_LT, OP_LE, OP_GT, OP_GE, OP_AND, OP_OR };
enum { UOP_NEG, UOP_NOT };
enum { P_IOTA, P_SHAPE, P_REVERSE, P_SUM, P_PRODUCT, P_MAX, P_MIN, P_LEN, P_PRINT, P_TAKE, P_DROP, P_KEEP, P_EACH };
enum { TY_INT = 1, TY_FLOAT, TY_BOOL, TY_CHAR, TY_STR, TY_INTVEC, TY_FLOATVEC, TY_BOOLVEC };

static int64_t g_live = 0;

void mim_panic(const char *msg) {
    fflush(stdout);
    fprintf(stderr, "mimic panic: %s\n", msg);
    exit(101);
}

static size_t elsize(int dt) {
    switch (dt) {
    case MIM_I64: return 8;
    case MIM_F64: return 8;
    case MIM_BOOL: return 1;
    default: return 1;
    }
}

static mim_val *alloc(int dtype, int rank, int64_t len) {
    mim_val *v = (mim_val *)malloc(sizeof *v);
    if (!v) mim_panic("out of memory");
    v->rc = 1;
    v->dtype = dtype;
    v->rank = rank;
    v->len = len;
    /* +1 byte so char vectors are always NUL-terminated for FFI */
    v->data = calloc((size_t)len * elsize(dtype) + 1, 1);
    if (!v->data) mim_panic("out of memory");
    g_live++;
    return v;
}

void mim_retain(mim_val *v) {
    if (v) v->rc++;
}

void mim_release(mim_val *v) {
    if (v && --v->rc == 0) {
        free(v->data);
        free(v);
        g_live--;
    }
}

static char **g_argv;
static int32_t g_argc;

void mim_rt_init(int32_t argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

int32_t mim_rt_shutdown(int32_t leak_check) {
    fflush(stdout);
    if (leak_check && g_live != 0) {
        fprintf(stderr, "mimic: leak check failed: %lld array(s) still alive at exit\n", (long long)g_live);
        return 102;
    }
    return 0;
}

/* ---- element access ---------------------------------------------------- */

static double getf(const mim_val *v, int64_t i) {
    switch (v->dtype) {
    case MIM_F64: return ((double *)v->data)[i];
    case MIM_I64: return (double)((int64_t *)v->data)[i];
    default: return (double)((uint8_t *)v->data)[i];
    }
}

static int64_t geti(const mim_val *v, int64_t i) {
    switch (v->dtype) {
    case MIM_F64: return (int64_t)((double *)v->data)[i];
    case MIM_I64: return ((int64_t *)v->data)[i];
    default: return (int64_t)((uint8_t *)v->data)[i];
    }
}

static void setf(mim_val *v, int64_t i, double x) {
    switch (v->dtype) {
    case MIM_F64: ((double *)v->data)[i] = x; break;
    case MIM_I64: ((int64_t *)v->data)[i] = (int64_t)x; break;
    default: ((uint8_t *)v->data)[i] = (uint8_t)x; break;
    }
}

static void seti(mim_val *v, int64_t i, int64_t x) {
    switch (v->dtype) {
    case MIM_F64: ((double *)v->data)[i] = (double)x; break;
    case MIM_I64: ((int64_t *)v->data)[i] = x; break;
    case MIM_BOOL: ((uint8_t *)v->data)[i] = x != 0; break;
    default: ((uint8_t *)v->data)[i] = (uint8_t)x; break;
    }
}

static const char *dtname(const mim_val *v) {
    static const char *s[] = {"i64", "f64", "bool", "char"};
    static const char *vs[] = {"[i64]", "[f64]", "[bool]", "str"};
    if (!v) return "()";
    return v->rank == 0 ? s[v->dtype] : vs[v->dtype];
}

static void need(const mim_val *v, const char *what) {
    if (!v) {
        char buf[160];
        snprintf(buf, sizeof buf, "%s: got unit value `()`", what);
        mim_panic(buf);
    }
}

/* ---- constructors ------------------------------------------------------ */

mim_val *mim_int(int64_t x) {
    mim_val *v = alloc(MIM_I64, 0, 1);
    ((int64_t *)v->data)[0] = x;
    return v;
}

mim_val *mim_float(double x) {
    mim_val *v = alloc(MIM_F64, 0, 1);
    ((double *)v->data)[0] = x;
    return v;
}

mim_val *mim_bool(int64_t x) {
    mim_val *v = alloc(MIM_BOOL, 0, 1);
    ((uint8_t *)v->data)[0] = x != 0;
    return v;
}

mim_val *mim_str(const char *s, int64_t len) {
    mim_val *v = alloc(MIM_CHAR, 1, len);
    memcpy(v->data, s, (size_t)len);
    return v;
}

mim_val *mim_from_cstr(const char *s) {
    if (!s) return mim_str("", 0);
    return mim_str(s, (int64_t)strlen(s));
}

mim_val *mim_array_lit(int64_t n, mim_val **elems) {
    if (n == 0) return alloc(MIM_I64, 1, 0);
    int dt = -1;
    for (int64_t i = 0; i < n; i++) {
        need(elems[i], "array literal");
        if (elems[i]->rank != 0) mim_panic("nested arrays are not supported yet (array literal elements must be scalars)");
        int e = elems[i]->dtype;
        if (dt == -1) dt = e;
        else if (dt != e) dt = (dt == MIM_F64 || e == MIM_F64) ? MIM_F64 : MIM_I64;
    }
    mim_val *v = alloc(dt, 1, n);
    for (int64_t i = 0; i < n; i++) {
        if (dt == MIM_F64) setf(v, i, getf(elems[i], 0));
        else seti(v, i, geti(elems[i], 0));
    }
    return v;
}

/* ---- operators --------------------------------------------------------- */

static int is_cmp(int op) { return op >= OP_EQ && op <= OP_OR; }

mim_val *mim_binop(int32_t op, mim_val *a, mim_val *b) {
    need(a, "operator");
    need(b, "operator");
    int64_t n;
    int rank;
    if (a->rank == 0 && b->rank == 0) { n = 1; rank = 0; }
    else if (a->rank == 0) { n = b->len; rank = 1; }
    else if (b->rank == 0) { n = a->len; rank = 1; }
    else {
        if (a->len != b->len) {
            char buf[128];
            snprintf(buf, sizeof buf, "length mismatch: %lld vs %lld", (long long)a->len, (long long)b->len);
            mim_panic(buf);
        }
        n = a->len;
        rank = 1;
    }
    int floaty = a->dtype == MIM_F64 || b->dtype == MIM_F64;
    int dt = is_cmp(op) ? MIM_BOOL : (op == OP_DIV || floaty) ? MIM_F64 : MIM_I64;
    mim_val *r = alloc(dt, rank, n);
    int64_t sa = a->rank == 0 ? 0 : 1, sb = b->rank == 0 ? 0 : 1;
    for (int64_t i = 0; i < n; i++) {
        int64_t ia = i * sa, ib = i * sb;
        if (floaty || op == OP_DIV) {
            double x = getf(a, ia), y = getf(b, ib);
            switch (op) {
            case OP_ADD: setf(r, i, x + y); break;
            case OP_SUB: setf(r, i, x - y); break;
            case OP_MUL: setf(r, i, x * y); break;
            case OP_DIV: setf(r, i, x / y); break;
            case OP_MOD: setf(r, i, fmod(x, y)); break;
            case OP_EQ: seti(r, i, x == y); break;
            case OP_NE: seti(r, i, x != y); break;
            case OP_LT: seti(r, i, x < y); break;
            case OP_LE: seti(r, i, x <= y); break;
            case OP_GT: seti(r, i, x > y); break;
            case OP_GE: seti(r, i, x >= y); break;
            case OP_AND: seti(r, i, x != 0 && y != 0); break;
            case OP_OR: seti(r, i, x != 0 || y != 0); break;
            }
        } else {
            int64_t x = geti(a, ia), y = geti(b, ib);
            switch (op) {
            case OP_ADD: seti(r, i, (int64_t)((uint64_t)x + (uint64_t)y)); break;
            case OP_SUB: seti(r, i, (int64_t)((uint64_t)x - (uint64_t)y)); break;
            case OP_MUL: seti(r, i, (int64_t)((uint64_t)x * (uint64_t)y)); break;
            case OP_MOD:
                if (y == 0) mim_panic("integer modulo by zero");
                seti(r, i, ((x % y) + y) % y);
                break;
            case OP_EQ: seti(r, i, x == y); break;
            case OP_NE: seti(r, i, x != y); break;
            case OP_LT: seti(r, i, x < y); break;
            case OP_LE: seti(r, i, x <= y); break;
            case OP_GT: seti(r, i, x > y); break;
            case OP_GE: seti(r, i, x >= y); break;
            case OP_AND: seti(r, i, x && y); break;
            case OP_OR: seti(r, i, x || y); break;
            }
        }
    }
    return r;
}

mim_val *mim_unop(int32_t op, mim_val *a) {
    need(a, "operator");
    mim_val *r = alloc(op == UOP_NOT ? MIM_BOOL : (a->dtype == MIM_F64 ? MIM_F64 : MIM_I64), a->rank, a->len);
    for (int64_t i = 0; i < a->len; i++) {
        if (op == UOP_NOT) seti(r, i, getf(a, i) == 0);
        else if (a->dtype == MIM_F64) setf(r, i, -getf(a, i));
        else seti(r, i, -geti(a, i));
    }
    return r;
}

/* ---- printing ---------------------------------------------------------- */

static void print_elem(const mim_val *v, int64_t i) {
    switch (v->dtype) {
    case MIM_I64: printf("%lld", (long long)geti(v, i)); break;
    case MIM_BOOL: printf("%s", geti(v, i) ? "true" : "false"); break;
    case MIM_CHAR: putchar((int)geti(v, i)); break;
    case MIM_F64: {
        char buf[64];
        double x = getf(v, i);
        snprintf(buf, sizeof buf, "%.15g", x);
        fputs(buf, stdout);
        if (isfinite(x) && !strpbrk(buf, ".e")) fputs(".0", stdout);
        break;
    }
    }
}

static void print_val(const mim_val *v) {
    if (!v) { fputs("()", stdout); return; }
    if (v->rank == 0) { print_elem(v, 0); return; }
    if (v->dtype == MIM_CHAR) { fwrite(v->data, 1, (size_t)v->len, stdout); return; }
    putchar('[');
    for (int64_t i = 0; i < v->len; i++) {
        if (i) putchar(' ');
        print_elem(v, i);
    }
    putchar(']');
}

/* ---- primitives -------------------------------------------------------- */

static mim_val *reduce(int prim, mim_val *a) {
    if (a->rank == 0) { mim_retain(a); return a; }
    int f = a->dtype == MIM_F64;
    if (a->len == 0) {
        if (prim == P_SUM) return mim_int(0);
        if (prim == P_PRODUCT) return mim_int(1);
        mim_panic("maximum/minimum of an empty array");
    }
    if (f) {
        double acc = getf(a, 0);
        for (int64_t i = 1; i < a->len; i++) {
            double x = getf(a, i);
            switch (prim) {
            case P_SUM: acc += x; break;
            case P_PRODUCT: acc *= x; break;
            case P_MAX: acc = x > acc ? x : acc; break;
            case P_MIN: acc = x < acc ? x : acc; break;
            }
        }
        return mim_float(acc);
    }
    int64_t acc = geti(a, 0);
    for (int64_t i = 1; i < a->len; i++) {
        int64_t x = geti(a, i);
        switch (prim) {
        case P_SUM: acc = (int64_t)((uint64_t)acc + (uint64_t)x); break;
        case P_PRODUCT: acc = (int64_t)((uint64_t)acc * (uint64_t)x); break;
        case P_MAX: acc = x > acc ? x : acc; break;
        case P_MIN: acc = x < acc ? x : acc; break;
        }
    }
    return mim_int(acc);
}

static mim_val *slice(mim_val *a, int64_t from, int64_t n) {
    mim_val *r = alloc(a->dtype, 1, n);
    memcpy(r->data, (char *)a->data + from * elsize(a->dtype), (size_t)n * elsize(a->dtype));
    return r;
}

mim_val *mim_prim1(int32_t prim, mim_val *a) {
    if (prim == P_PRINT) {
        print_val(a);
        putchar('\n');
        return NULL;
    }
    need(a, "primitive");
    switch (prim) {
    case P_IOTA: {
        if (a->rank != 0 || a->dtype == MIM_F64) mim_panic("⍳ (iota) expects an integer scalar");
        int64_t n = geti(a, 0);
        if (n < 0) mim_panic("⍳ (iota) of a negative number");
        mim_val *r = alloc(MIM_I64, 1, n);
        for (int64_t i = 0; i < n; i++) ((int64_t *)r->data)[i] = i;
        return r;
    }
    case P_SHAPE: {
        if (a->rank == 0) return alloc(MIM_I64, 1, 0);
        mim_val *r = alloc(MIM_I64, 1, 1);
        ((int64_t *)r->data)[0] = a->len;
        return r;
    }
    case P_REVERSE: {
        if (a->rank == 0) { mim_retain(a); return a; }
        mim_val *r = alloc(a->dtype, 1, a->len);
        size_t es = elsize(a->dtype);
        for (int64_t i = 0; i < a->len; i++)
            memcpy((char *)r->data + i * es, (char *)a->data + (a->len - 1 - i) * es, es);
        return r;
    }
    case P_SUM:
    case P_PRODUCT:
    case P_MAX:
    case P_MIN: return reduce(prim, a);
    case P_LEN: return mim_int(a->rank == 0 ? 1 : a->len);
    }
    mim_panic("unknown monadic primitive");
    return NULL;
}

mim_val *mim_prim2(int32_t prim, mim_val *a, mim_val *b) {
    need(a, "primitive");
    need(b, "primitive");
    switch (prim) {
    case P_TAKE:
    case P_DROP: {
        if (b->rank != 0 || b->dtype == MIM_F64) mim_panic("take/drop count must be an integer scalar");
        if (a->rank == 0) mim_panic("take/drop expects a vector on the left");
        int64_t n = geti(b, 0), len = a->len;
        int64_t k = n < 0 ? -n : n;
        if (k > len) k = len;
        if (prim == P_TAKE) return n >= 0 ? slice(a, 0, k) : slice(a, len - k, k);
        return n >= 0 ? slice(a, k, len - k) : slice(a, 0, len - k);
    }
    case P_KEEP: {
        if (a->rank == 0 || b->rank == 0) mim_panic("⌿ (keep) expects two vectors");
        if (a->len != b->len) mim_panic("⌿ (keep): length mismatch");
        int64_t total = 0;
        for (int64_t i = 0; i < b->len; i++) {
            int64_t c = geti(b, i);
            if (c < 0) mim_panic("⌿ (keep): negative count");
            total += c;
        }
        mim_val *r = alloc(a->dtype, 1, total);
        size_t es = elsize(a->dtype);
        int64_t o = 0;
        for (int64_t i = 0; i < a->len; i++)
            for (int64_t c = geti(b, i); c > 0; c--, o++)
                memcpy((char *)r->data + o * es, (char *)a->data + i * es, es);
        return r;
    }
    }
    mim_panic("unknown dyadic primitive");
    return NULL;
}

static mim_val *scalar_at(mim_val *xs, int64_t i) {
    switch (xs->dtype) {
    case MIM_F64: return mim_float(getf(xs, i));
    case MIM_BOOL: return mim_bool(geti(xs, i));
    case MIM_CHAR: {
        mim_val *v = alloc(MIM_CHAR, 0, 1);
        ((uint8_t *)v->data)[0] = (uint8_t)geti(xs, i);
        return v;
    }
    default: return mim_int(geti(xs, i));
    }
}

mim_val *mim_each(mim_val *xs, mim_fn1 f) {
    need(xs, "¨ (each)");
    if (xs->rank == 0) return f(xs);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)(xs->len ? xs->len : 1));
    for (int64_t i = 0; i < xs->len; i++) {
        mim_val *e = scalar_at(xs, i);
        tmp[i] = f(e);
        mim_release(e);
        need(tmp[i], "¨ (each) function result");
        if (tmp[i]->rank != 0) mim_panic("¨ (each): the function must return a scalar (nested arrays come later)");
    }
    mim_val *r = xs->len ? mim_array_lit(xs->len, tmp) : alloc(MIM_I64, 1, 0);
    for (int64_t i = 0; i < xs->len; i++) mim_release(tmp[i]);
    free(tmp);
    return r;
}

static int64_t check_index(mim_val *xs, mim_val *i) {
    need(xs, "index");
    need(i, "index");
    if (xs->rank == 0) mim_panic("cannot index a scalar");
    if (i->rank != 0 || i->dtype == MIM_F64) mim_panic("index must be an integer scalar");
    int64_t k = geti(i, 0);
    if (k < 0 || k >= xs->len) {
        char buf[128];
        snprintf(buf, sizeof buf, "index %lld out of bounds for length %lld", (long long)k, (long long)xs->len);
        mim_panic(buf);
    }
    return k;
}

mim_val *mim_index(mim_val *xs, mim_val *i) {
    return scalar_at(xs, check_index(xs, i));
}

/* Copy-on-write element store: mutates in place when uniquely owned. */
void mim_set_index(mim_val **slot, mim_val *i, mim_val *v) {
    mim_val *xs = *slot;
    int64_t k = check_index(xs, i);
    need(v, "indexed assignment");
    if (v->rank != 0) mim_panic("indexed assignment: value must be a scalar");
    int newdt = xs->dtype;
    if (v->dtype == MIM_F64 && xs->dtype != MIM_F64) newdt = MIM_F64;
    if (xs->rc > 1 || newdt != xs->dtype) {
        mim_val *c = alloc(newdt, 1, xs->len);
        for (int64_t j = 0; j < xs->len; j++) {
            if (newdt == MIM_F64) setf(c, j, getf(xs, j));
            else seti(c, j, geti(xs, j));
        }
        mim_release(xs);
        *slot = xs = c;
    }
    if (xs->dtype == MIM_F64) setf(xs, k, getf(v, 0));
    else seti(xs, k, geti(v, 0));
}

int64_t mim_truthy(mim_val *v) {
    need(v, "condition");
    if (v->rank != 0) mim_panic("condition must be a scalar, found an array (reduce it first, e.g. with ⌈/)");
    return getf(v, 0) != 0;
}

void mim_check_type(mim_val *v, int32_t ty, const char *what) {
    int ok = 0;
    static const char *names[] = {"", "integer scalar", "f64 scalar", "bool", "char", "str", "[i64]", "[f64]", "[bool]"};
    if (v) {
        int s = v->rank == 0;
        switch (ty) {
        case TY_INT: ok = s && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL); break;
        case TY_FLOAT: ok = s && v->dtype != MIM_CHAR; break;
        case TY_BOOL: ok = s && v->dtype == MIM_BOOL; break;
        case TY_CHAR: ok = s && v->dtype == MIM_CHAR; break;
        case TY_STR: ok = !s && v->dtype == MIM_CHAR; break;
        case TY_INTVEC: ok = !s && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL); break;
        case TY_FLOATVEC: ok = !s && v->dtype != MIM_CHAR; break;
        case TY_BOOLVEC: ok = !s && v->dtype == MIM_BOOL; break;
        }
    }
    if (!ok) {
        char buf[256];
        snprintf(buf, sizeof buf, "type error: %s expected %s, found %s", what, names[ty], dtname(v));
        mim_panic(buf);
    }
}

/* ---- FFI marshalling --------------------------------------------------- */

int64_t mim_as_i64(mim_val *v) {
    need(v, "FFI argument");
    if (v->rank != 0) mim_panic("FFI: expected a scalar argument");
    return geti(v, 0);
}

double mim_as_f64(mim_val *v) {
    need(v, "FFI argument");
    if (v->rank != 0) mim_panic("FFI: expected a scalar argument");
    return getf(v, 0);
}

const char *mim_cstr(mim_val *v) {
    need(v, "FFI argument");
    if (v->dtype != MIM_CHAR || v->rank != 1) mim_panic("FFI: cstr parameter expects a string");
    return (const char *)v->data;
}

void *mim_dataptr(mim_val *v) {
    need(v, "FFI argument");
    return v->data;
}
