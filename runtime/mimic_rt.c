/* Mimic runtime — array layer. See mimic_rt.h for the ownership contract.
 *
 * Milestone 2 (phase 1): refcounts are atomic and bulk kernels run on a
 * thread pool. Work is split into fixed-size chunks (independent of the
 * thread count), so results — including floating-point reductions — are
 * bit-identical for any number of threads.
 *
 * Tuning (environment):
 *   MIMIC_THREADS     worker count (default: online CPUs; 1 = sequential)
 *   MIMIC_PAR_GRAIN   elements per chunk (default 16384)
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__wasi__) || defined(MIMIC_NO_THREADS)
#define MIM_THREADS 0
#elif defined(_WIN32)
#define MIM_THREADS 1
#include <windows.h>
#else
#define MIM_THREADS 1
#include <pthread.h>
#include <sched.h>
#include <unistd.h>
#endif

typedef enum { MIM_I64 = 0, MIM_F64 = 1, MIM_BOOL = 2, MIM_CHAR = 3 } mim_dtype;

typedef struct mim_val {
    int64_t rc; /* atomic; >= MIM_IMMORTAL means never freed */
    int32_t dtype;
    int32_t rank;
    int64_t len;
    void *data; /* points at `inl` (one allocation per value) */
    int64_t inl[];
} mim_val;

#define MIM_IMMORTAL ((int64_t)1 << 62)

typedef mim_val *(*mim_fn1)(mim_val *);

enum { OP_ADD, OP_SUB, OP_MUL, OP_DIV, OP_MOD, OP_EQ, OP_NE, OP_LT, OP_LE, OP_GT, OP_GE, OP_AND, OP_OR, OP_IDIV, OP_MAX2, OP_MIN2 };
enum { UOP_NEG, UOP_NOT, UOP_ABS };
enum { P_IOTA, P_SHAPE, P_REVERSE, P_SUM, P_PRODUCT, P_MAX, P_MIN, P_LEN, P_PRINT, P_TAKE, P_DROP, P_KEEP, P_EACH, P_RANGE };

void mim_panic(const char *msg);

/* floored integer division (matches `%`): 7 div 2 = 3, -7 div 2 = -4 */
static int64_t floordiv(int64_t x, int64_t y) {
    if (y == 0) mim_panic("integer division by zero");
    int64_t q = x / y;
    if ((x % y != 0) && ((x < 0) != (y < 0))) q--;
    return q;
}
enum { TY_INT = 1, TY_FLOAT, TY_BOOL, TY_CHAR, TY_STR, TY_INTVEC, TY_FLOATVEC, TY_BOOLVEC };

static void park_forever(void);

void mim_panic(const char *msg) {
    /* Several worker threads may panic at once: only the first reports and
     * exits (exit() is not safe to call concurrently); the rest park. */
    static int panicking = 0;
    if (__atomic_exchange_n(&panicking, 1, __ATOMIC_ACQ_REL)) park_forever();
    fflush(stdout);
    fprintf(stderr, "mimic panic: %s\n", msg);
    exit(101);
}

/* ==== thread pool ======================================================== */

typedef void (*mim_task)(void *ctx, int64_t lo, int64_t hi);

static int64_t g_grain = 16384;
static int g_nthreads = 1;
static _Thread_local int t_in_parallel = 0;

#if MIM_THREADS
#ifdef _WIN32
typedef SRWLOCK mim_mutex;
typedef CONDITION_VARIABLE mim_cond;
#define mutex_init(m) InitializeSRWLock(m)
#define mutex_lock(m) AcquireSRWLockExclusive(m)
#define mutex_unlock(m) ReleaseSRWLockExclusive(m)
#define mutex_trylock(m) TryAcquireSRWLockExclusive(m)
#define cond_init(c) InitializeConditionVariable(c)
#define cond_wait(c, m) SleepConditionVariableSRW(c, m, INFINITE, 0)
#define cond_broadcast(c) WakeAllConditionVariable(c)
#else
typedef pthread_mutex_t mim_mutex;
typedef pthread_cond_t mim_cond;
#define mutex_init(m) pthread_mutex_init(m, NULL)
#define mutex_lock(m) pthread_mutex_lock(m)
#define mutex_unlock(m) pthread_mutex_unlock(m)
#define mutex_trylock(m) (pthread_mutex_trylock(m) == 0)
#define cond_init(c) pthread_cond_init(c, NULL)
#define cond_wait(c, m) pthread_cond_wait(c, m)
#define cond_broadcast(c) pthread_cond_broadcast(c)
#endif

/* One job at a time, published by the main thread. Workers that pick it up
 * register in `active`, so the job (on the caller's stack) outlives them. */
typedef struct {
    mim_task fn;
    void *ctx;
    int64_t n, nchunks, grain;
    int64_t next;      /* atomic: next chunk to claim */
    int64_t remaining; /* atomic: chunks not yet finished */
    int64_t active;    /* workers attached (guarded by g_mu) */
} mim_job;

static mim_mutex g_mu;
static mim_cond g_work, g_done;
static mim_job *g_job = NULL;
static uint64_t g_gen = 0;
static int g_started = 0;

static void run_chunks(mim_job *j) {
    for (;;) {
        int64_t c = __atomic_fetch_add(&j->next, 1, __ATOMIC_RELAXED);
        if (c >= j->nchunks) return;
        int64_t lo = c * j->grain, hi = lo + j->grain;
        if (hi > j->n) hi = j->n;
        j->fn(j->ctx, lo, hi);
        if (__atomic_sub_fetch(&j->remaining, 1, __ATOMIC_ACQ_REL) == 0) {
            mutex_lock(&g_mu);
            cond_broadcast(&g_done);
            mutex_unlock(&g_mu);
        }
    }
}

#ifdef _WIN32
static DWORD WINAPI worker(LPVOID arg)
#else
static void *worker(void *arg)
#endif
{
    (void)arg;
    t_in_parallel = 1;
    uint64_t seen = 0;
    mutex_lock(&g_mu);
    for (;;) {
        while (g_gen == seen || g_job == NULL) {
            if (g_gen != seen) seen = g_gen; /* job already retired */
            cond_wait(&g_work, &g_mu);
        }
        seen = g_gen;
        mim_job *j = g_job;
        j->active++;
        mutex_unlock(&g_mu);
        run_chunks(j);
        mutex_lock(&g_mu);
        if (--j->active == 0) cond_broadcast(&g_done);
    }
    return 0;
}

static void start_pool(void) {
    mutex_init(&g_mu);
    cond_init(&g_work);
    cond_init(&g_done);
    for (int i = 1; i < g_nthreads; i++) {
#ifdef _WIN32
        HANDLE h = CreateThread(NULL, (SIZE_T)32 << 20, worker, NULL, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
        if (h) CloseHandle(h);
#else
        pthread_t t;
        pthread_attr_t a;
        pthread_attr_init(&a);
        pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
        /* native code may recurse (up to NATIVE_LIMIT frames) on workers */
        pthread_attr_setstacksize(&a, (size_t)32 << 20);
        pthread_create(&t, &a, worker, NULL);
        pthread_attr_destroy(&a);
#endif
    }
    g_started = 1;
}
#else /* no threads: locks are no-ops */
typedef int mim_mutex;
#define mutex_init(m) ((void)(m))
#define mutex_lock(m) ((void)(m))
#define mutex_unlock(m) ((void)(m))
#define mutex_trylock(m) ((void)(m), 1)
#endif /* MIM_THREADS */

/* Run fn over [0, n) in chunks of `grain`, in parallel when worthwhile.
 * Nested calls (from inside a parallel task) run sequentially. */
static void parallel_for_grain(int64_t n, int64_t grain, mim_task fn, void *ctx) {
    if (n <= 0) return;
    if (grain < 1) grain = 1;
    int64_t nchunks = (n + grain - 1) / grain;
#if MIM_THREADS
    if (g_nthreads > 1 && nchunks > 1 && !t_in_parallel) {
        if (!g_started) start_pool();
        mim_job j = {fn, ctx, n, nchunks, grain, 0, nchunks, 0};
        mutex_lock(&g_mu);
        g_job = &j;
        g_gen++;
        cond_broadcast(&g_work);
        mutex_unlock(&g_mu);
        t_in_parallel = 1;
        run_chunks(&j);
        t_in_parallel = 0;
        mutex_lock(&g_mu);
        while (__atomic_load_n(&j.remaining, __ATOMIC_ACQUIRE) > 0) cond_wait(&g_done, &g_mu);
        g_job = NULL; /* no new worker may attach */
        while (j.active > 0) cond_wait(&g_done, &g_mu);
        mutex_unlock(&g_mu);
        return;
    }
#endif
    int saved = t_in_parallel;
    t_in_parallel = 1; /* keep the semantics identical to the parallel path */
    for (int64_t c = 0; c < nchunks; c++) {
        int64_t lo = c * grain, hi = lo + grain;
        fn(ctx, lo, hi > n ? n : hi);
    }
    t_in_parallel = saved;
}

/* Fixed-size chunks: chunk boundaries never depend on the thread count, so
 * reductions are bit-reproducible. Used by the data-parallel kernels. */
static void parallel_for(int64_t n, mim_task fn, void *ctx) { parallel_for_grain(n, g_grain, fn, ctx); }

static int g_grain_forced = 0;

/* For expensive per-element work (calling compiled functions): aim for ~16
 * chunks per thread so the load balances even when elements cost unevenly. */
static int64_t fine_grain(int64_t n) {
    if (g_grain_forced) return g_grain;
    int64_t g = n / ((int64_t)g_nthreads * 16);
    return g < 1 ? 1 : g;
}

static void park_forever(void) {
    for (;;) {
#if MIM_THREADS && defined(_WIN32)
        Sleep(INFINITE);
#elif MIM_THREADS
        sleep(1000);
#endif
    }
}

static int64_t nchunks_of(int64_t n) { return n <= 0 ? 0 : (n + g_grain - 1) / g_grain; }

/* ==== allocation & refcounting ===========================================
 *
 * - one malloc per value: header and elements live in the same block;
 * - small blocks (scalars, short strings) are recycled through a
 *   per-thread free list, so the hot path never touches the global heap;
 * - the live-array count used by the leak check is kept per thread
 *   (cache-line padded) instead of in one contended atomic.
 */

static size_t elsize(int dt) {
    switch (dt) {
    case MIM_I64: return 8;
    case MIM_F64: return 8;
    default: return 1;
    }
}

#define SMALL_BYTES 16 /* payload bytes (incl. NUL) served by the free list */
#define FREE_LIST_CAP 4096
#define MAX_SLOTS 257

/* per-thread counters, padded to 128-byte lines (Apple Silicon) */
typedef struct { int64_t n; int64_t nodes; int64_t itrs; char pad[104]; } padded_count;
static padded_count g_live_slots[MAX_SLOTS];
static int g_next_slot = 0;
static _Thread_local int t_slot = -1;
static _Thread_local mim_val *t_free = NULL;
static _Thread_local int t_free_len = 0;

static padded_count *live_slot(void) {
    if (t_slot < 0) {
        int s = __atomic_fetch_add(&g_next_slot, 1, __ATOMIC_RELAXED);
        t_slot = s < MAX_SLOTS ? s : MAX_SLOTS - 1;
    }
    return &g_live_slots[t_slot];
}

static mim_val *alloc(int dtype, int rank, int64_t len) {
    size_t bytes = (size_t)len * elsize(dtype) + 1; /* +1: char vectors stay NUL-terminated for FFI */
    mim_val *v;
    if (bytes <= SMALL_BYTES && t_free) {
        v = t_free;
        t_free = *(mim_val **)v;
        t_free_len--;
    } else {
        v = (mim_val *)malloc(sizeof(mim_val) + (bytes <= SMALL_BYTES ? SMALL_BYTES : bytes));
        if (!v) mim_panic("out of memory");
    }
    v->rc = 1;
    v->dtype = dtype;
    v->rank = rank;
    v->len = len;
    v->data = v->inl;
    ((char *)v->data)[bytes - 1] = 0;
    __atomic_store_n(&live_slot()->n, live_slot()->n + 1, __ATOMIC_RELAXED);
    return v;
}

static void dealloc(mim_val *v) {
    size_t bytes = (size_t)v->len * elsize(v->dtype) + 1;
    __atomic_store_n(&live_slot()->n, live_slot()->n - 1, __ATOMIC_RELAXED);
    if (bytes <= SMALL_BYTES && t_free_len < FREE_LIST_CAP) {
        *(mim_val **)v = t_free;
        t_free = v;
        t_free_len++;
    } else {
        free(v);
    }
}

void mim_retain(mim_val *v) {
    if (v && __atomic_load_n(&v->rc, __ATOMIC_RELAXED) < MIM_IMMORTAL) __atomic_fetch_add(&v->rc, 1, __ATOMIC_RELAXED);
}

void mim_release(mim_val *v) {
    if (v && __atomic_load_n(&v->rc, __ATOMIC_RELAXED) < MIM_IMMORTAL && __atomic_sub_fetch(&v->rc, 1, __ATOMIC_ACQ_REL) == 0)
        dealloc(v);
}

/* Immortal values are never counted, refcounted or freed. */
static mim_val *immortal(mim_val *v) {
    __atomic_store_n(&live_slot()->n, live_slot()->n - 1, __ATOMIC_RELAXED);
    v->rc = MIM_IMMORTAL;
    return v;
}

#define SMALL_INT_MIN (-128)
#define SMALL_INT_MAX 1023
static mim_val *g_small_ints[SMALL_INT_MAX - SMALL_INT_MIN + 1];
static mim_val *g_true, *g_false;

static char **g_argv;
static int32_t g_argc;

void mim_rt_init(int32_t argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
    for (int64_t i = SMALL_INT_MIN; i <= SMALL_INT_MAX; i++) {
        mim_val *v = immortal(alloc(MIM_I64, 0, 1));
        ((int64_t *)v->data)[0] = i;
        g_small_ints[i - SMALL_INT_MIN] = v;
    }
    g_true = immortal(alloc(MIM_BOOL, 0, 1));
    ((uint8_t *)g_true->data)[0] = 1;
    g_false = immortal(alloc(MIM_BOOL, 0, 1));
    ((uint8_t *)g_false->data)[0] = 0;
    const char *t = getenv("MIMIC_THREADS");
    const char *g = getenv("MIMIC_PAR_GRAIN");
    if (g && atoll(g) > 0) {
        g_grain = atoll(g);
        g_grain_forced = 1;
    }
#if MIM_THREADS
    if (t && atoi(t) > 0) {
        g_nthreads = atoi(t);
    } else {
#ifdef _WIN32
        SYSTEM_INFO si;
        GetSystemInfo(&si);
        g_nthreads = (int)si.dwNumberOfProcessors;
#else
        long n = sysconf(_SC_NPROCESSORS_ONLN);
        g_nthreads = n > 0 ? (int)n : 1;
#endif
    }
    if (g_nthreads > 256) g_nthreads = 256;
#else
    (void)t;
    g_nthreads = 1;
#endif
}

int32_t mim_rt_shutdown(int32_t leak_check) {
    fflush(stdout);
    int64_t live = 0;
    for (int i = 0; i < MAX_SLOTS; i++) live += __atomic_load_n(&g_live_slots[i].n, __ATOMIC_ACQUIRE);
    if (leak_check && live != 0) {
        fprintf(stderr, "mimic: leak check failed: %lld array(s) still alive at exit\n", (long long)live);
        return 102;
    }
    int64_t nodes = 0;
    for (int i = 0; i < MAX_SLOTS; i++) nodes += __atomic_load_n(&g_live_slots[i].nodes, __ATOMIC_ACQUIRE);
    if (leak_check && nodes != 0) {
        fprintf(stderr, "mimic: leak check failed: %lld interaction-net node(s) still alive at exit\n", (long long)nodes);
        return 102;
    }
    return 0;
}

/* ==== element access ===================================================== */

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

/* ==== constructors ======================================================= */

mim_val *mim_int(int64_t x) {
    if (x >= SMALL_INT_MIN && x <= SMALL_INT_MAX && g_small_ints[0]) return g_small_ints[x - SMALL_INT_MIN];
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
    if (g_true) return x ? g_true : g_false;
    mim_val *v = alloc(MIM_BOOL, 0, 1);
    ((uint8_t *)v->data)[0] = x != 0;
    return v;
}

/* Literals: created once, immortal, cached in a compiler-emitted slot. */
static mim_val *publish(mim_val **slot, mim_val *v) {
    v = immortal(v);
    mim_val *expected = NULL;
    if (!__atomic_compare_exchange_n(slot, &expected, v, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        free(v); /* another thread won the race */
        return expected;
    }
    return v;
}

mim_val *mim_lit_int(mim_val **slot, int64_t x) {
    mim_val *v = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
    if (v) return v;
    mim_val *n = alloc(MIM_I64, 0, 1);
    ((int64_t *)n->data)[0] = x;
    return publish(slot, n);
}

mim_val *mim_lit_float(mim_val **slot, double x) {
    mim_val *v = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
    if (v) return v;
    mim_val *n = alloc(MIM_F64, 0, 1);
    ((double *)n->data)[0] = x;
    return publish(slot, n);
}

mim_val *mim_lit_str(mim_val **slot, const char *s, int64_t len) {
    mim_val *v = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
    if (v) return v;
    mim_val *n = alloc(MIM_CHAR, 1, len);
    memcpy(n->data, s, (size_t)len);
    return publish(slot, n);
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

static int join_dtype(int a, int b) {
    if (a == b) return a;
    return (a == MIM_F64 || b == MIM_F64) ? MIM_F64 : MIM_I64;
}

mim_val *mim_array_lit(int64_t n, mim_val **elems) {
    if (n == 0) return alloc(MIM_I64, 1, 0);
    int dt = -1;
    for (int64_t i = 0; i < n; i++) {
        need(elems[i], "array literal");
        if (elems[i]->rank != 0) mim_panic("nested arrays are not supported yet (array literal elements must be scalars)");
        dt = dt == -1 ? elems[i]->dtype : join_dtype(dt, elems[i]->dtype);
    }
    mim_val *v = alloc(dt, 1, n);
    for (int64_t i = 0; i < n; i++) {
        if (dt == MIM_F64) setf(v, i, getf(elems[i], 0));
        else seti(v, i, geti(elems[i], 0));
    }
    return v;
}

/* ==== elementwise operators ============================================== */

static int is_cmp(int op) { return op >= OP_EQ && op <= OP_OR; }

typedef struct {
    int op;
    const mim_val *a, *b;
    mim_val *r;
    int64_t sa, sb; /* 0 = broadcast scalar, 1 = vector */
} binop_ctx;

/* Fast typed loops (vectorisable) for the common same-dtype cases. */
#define ARITH_LOOP(T, OUT, EXPR)                                                                   \
    do {                                                                                           \
        const T *x = (const T *)c->a->data, *y = (const T *)c->b->data;                            \
        OUT *z = (OUT *)c->r->data;                                                                \
        if (c->sa && c->sb) for (int64_t i = lo; i < hi; i++) { T X = x[i], Y = y[i]; z[i] = (OUT)(EXPR); } \
        else if (c->sa) { T Y = y[0]; for (int64_t i = lo; i < hi; i++) { T X = x[i]; z[i] = (OUT)(EXPR); } } \
        else { T X = x[0]; for (int64_t i = lo; i < hi; i++) { T Y = y[i]; z[i] = (OUT)(EXPR); } } \
    } while (0)

static int binop_fast(binop_ctx *c, int64_t lo, int64_t hi) {
    int dt = c->a->dtype;
    if (dt != c->b->dtype) return 0;
    if (dt == MIM_F64) {
        switch (c->op) {
        case OP_ADD: ARITH_LOOP(double, double, X + Y); return 1;
        case OP_SUB: ARITH_LOOP(double, double, X - Y); return 1;
        case OP_MUL: ARITH_LOOP(double, double, X * Y); return 1;
        case OP_DIV: ARITH_LOOP(double, double, X / Y); return 1;
        case OP_LT: ARITH_LOOP(double, uint8_t, X < Y); return 1;
        case OP_GT: ARITH_LOOP(double, uint8_t, X > Y); return 1;
        case OP_EQ: ARITH_LOOP(double, uint8_t, X == Y); return 1;
        }
    } else if (dt == MIM_I64) {
        switch (c->op) {
        case OP_ADD: ARITH_LOOP(int64_t, int64_t, (int64_t)((uint64_t)X + (uint64_t)Y)); return 1;
        case OP_SUB: ARITH_LOOP(int64_t, int64_t, (int64_t)((uint64_t)X - (uint64_t)Y)); return 1;
        case OP_MUL: ARITH_LOOP(int64_t, int64_t, (int64_t)((uint64_t)X * (uint64_t)Y)); return 1;
        case OP_LT: ARITH_LOOP(int64_t, uint8_t, X < Y); return 1;
        case OP_GT: ARITH_LOOP(int64_t, uint8_t, X > Y); return 1;
        case OP_EQ: ARITH_LOOP(int64_t, uint8_t, X == Y); return 1;
        }
    }
    return 0;
}

static void binop_range(void *vc, int64_t lo, int64_t hi) {
    binop_ctx *c = (binop_ctx *)vc;
    if (binop_fast(c, lo, hi)) return;
    int op = c->op;
    int floaty = c->a->dtype == MIM_F64 || c->b->dtype == MIM_F64;
    for (int64_t i = lo; i < hi; i++) {
        int64_t ia = i * c->sa, ib = i * c->sb;
        mim_val *r = c->r;
        if (floaty || op == OP_DIV) { /* note: OP_IDIV on ints stays integer */
            double x = getf(c->a, ia), y = getf(c->b, ib);
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
            case OP_IDIV: setf(r, i, floor(x / y)); break;
            case OP_MAX2: setf(r, i, x > y ? x : y); break;
            case OP_MIN2: setf(r, i, x < y ? x : y); break;
            }
        } else {
            int64_t x = geti(c->a, ia), y = geti(c->b, ib);
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
            case OP_IDIV: seti(r, i, floordiv(x, y)); break;
            case OP_MAX2: seti(r, i, x > y ? x : y); break;
            case OP_MIN2: seti(r, i, x < y ? x : y); break;
            }
        }
    }
}

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
    binop_ctx c = {op, a, b, r, a->rank == 0 ? 0 : 1, b->rank == 0 ? 0 : 1};
    /* the fast loops index x[i] for vectors and x[0] for scalars */
    parallel_for(n, binop_range, &c);
    return r;
}

typedef struct { int op; const mim_val *a; mim_val *r; } unop_ctx;

static void unop_range(void *vc, int64_t lo, int64_t hi) {
    unop_ctx *c = (unop_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) {
        if (c->op == UOP_NOT) seti(c->r, i, getf(c->a, i) == 0);
        else if (c->op == UOP_ABS) {
            if (c->a->dtype == MIM_F64) setf(c->r, i, fabs(getf(c->a, i)));
            else { int64_t x = geti(c->a, i); seti(c->r, i, x < 0 ? -x : x); }
        }
        else if (c->a->dtype == MIM_F64) setf(c->r, i, -getf(c->a, i));
        else seti(c->r, i, -geti(c->a, i));
    }
}

mim_val *mim_unop(int32_t op, mim_val *a) {
    need(a, "operator");
    mim_val *r = alloc(op == UOP_NOT ? MIM_BOOL : (a->dtype == MIM_F64 ? MIM_F64 : MIM_I64), a->rank, a->len);
    unop_ctx c = {op, a, r};
    parallel_for(a->len, unop_range, &c);
    return r;
}

/* ==== printing =========================================================== */

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

/* ==== reductions ========================================================= */

/* Each chunk reduces into its own slot; slots are combined in chunk order,
 * so the result does not depend on the thread count. */
typedef struct {
    int prim;
    const mim_val *a;
    double *pf;
    int64_t *pi;
} reduce_ctx;

static void reduce_range(void *vc, int64_t lo, int64_t hi) {
    reduce_ctx *c = (reduce_ctx *)vc;
    int64_t slot = lo / g_grain;
    if (c->pf) {
        const double *x = (const double *)c->a->data;
        double acc = x[lo];
        switch (c->prim) {
        case P_SUM: for (int64_t i = lo + 1; i < hi; i++) acc += x[i]; break;
        case P_PRODUCT: for (int64_t i = lo + 1; i < hi; i++) acc *= x[i]; break;
        case P_MAX: for (int64_t i = lo + 1; i < hi; i++) acc = x[i] > acc ? x[i] : acc; break;
        case P_MIN: for (int64_t i = lo + 1; i < hi; i++) acc = x[i] < acc ? x[i] : acc; break;
        }
        c->pf[slot] = acc;
    } else {
        int64_t acc = geti(c->a, lo);
        for (int64_t i = lo + 1; i < hi; i++) {
            int64_t x = geti(c->a, i);
            switch (c->prim) {
            case P_SUM: acc = (int64_t)((uint64_t)acc + (uint64_t)x); break;
            case P_PRODUCT: acc = (int64_t)((uint64_t)acc * (uint64_t)x); break;
            case P_MAX: acc = x > acc ? x : acc; break;
            case P_MIN: acc = x < acc ? x : acc; break;
            }
        }
        c->pi[slot] = acc;
    }
}

static mim_val *reduce(int prim, mim_val *a) {
    if (a->rank == 0) { mim_retain(a); return a; }
    if (a->len == 0) {
        if (prim == P_SUM) return mim_int(0);
        if (prim == P_PRODUCT) return mim_int(1);
        mim_panic("maximum/minimum of an empty array");
    }
    int f = a->dtype == MIM_F64;
    int64_t nc = nchunks_of(a->len);
    reduce_ctx c = {prim, a, NULL, NULL};
    if (f) c.pf = (double *)malloc(sizeof(double) * (size_t)nc);
    else c.pi = (int64_t *)malloc(sizeof(int64_t) * (size_t)nc);
    parallel_for(a->len, reduce_range, &c);
    mim_val *r;
    if (f) {
        double acc = c.pf[0];
        for (int64_t k = 1; k < nc; k++) {
            double x = c.pf[k];
            switch (prim) {
            case P_SUM: acc += x; break;
            case P_PRODUCT: acc *= x; break;
            case P_MAX: acc = x > acc ? x : acc; break;
            case P_MIN: acc = x < acc ? x : acc; break;
            }
        }
        r = mim_float(acc);
        free(c.pf);
    } else {
        int64_t acc = c.pi[0];
        for (int64_t k = 1; k < nc; k++) {
            int64_t x = c.pi[k];
            switch (prim) {
            case P_SUM: acc = (int64_t)((uint64_t)acc + (uint64_t)x); break;
            case P_PRODUCT: acc = (int64_t)((uint64_t)acc * (uint64_t)x); break;
            case P_MAX: acc = x > acc ? x : acc; break;
            case P_MIN: acc = x < acc ? x : acc; break;
            }
        }
        r = mim_int(acc);
        free(c.pi);
    }
    return r;
}

/* ==== structural primitives ============================================== */

static mim_val *slice(mim_val *a, int64_t from, int64_t n) {
    mim_val *r = alloc(a->dtype, 1, n);
    memcpy(r->data, (char *)a->data + from * elsize(a->dtype), (size_t)n * elsize(a->dtype));
    return r;
}

static void iota_range(void *vc, int64_t lo, int64_t hi) {
    int64_t *d = (int64_t *)vc;
    for (int64_t i = lo; i < hi; i++) d[i] = i;
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
        parallel_for(n, iota_range, r->data);
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
    case P_RANGE: {
        if (a->rank != 0 || b->rank != 0 || a->dtype == MIM_F64 || b->dtype == MIM_F64)
            mim_panic("‥ (range) expects two integer scalars");
        int64_t lo = geti(a, 0), hi = geti(b, 0), n = hi > lo ? hi - lo : 0;
        mim_val *r = alloc(MIM_I64, 1, n);
        for (int64_t i = 0; i < n; i++) ((int64_t *)r->data)[i] = lo + i;
        return r;
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

/* ==== each =============================================================== */

static mim_val *scalar_at(const mim_val *xs, int64_t i) {
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

typedef struct {
    const mim_val *xs;
    mim_fn1 f;
    mim_val **out;
} each_ctx;

static void each_range(void *vc, int64_t lo, int64_t hi) {
    each_ctx *c = (each_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) {
        mim_val *e = scalar_at(c->xs, i);
        mim_val *r = c->f(e);
        mim_release(e);
        need(r, "¨ (each) function result");
        if (r->rank != 0) mim_panic("¨ (each): the function must return a scalar (nested arrays come later)");
        c->out[i] = r;
    }
}

/* `pure` functions (no print / FFI, transitively — decided by the compiler)
 * run in parallel; impure ones run in order on the calling thread. */
mim_val *mim_each(mim_val *xs, mim_fn1 f, int32_t pure) {
    need(xs, "¨ (each)");
    if (xs->rank == 0) return f(xs);
    int64_t n = xs->len;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    each_ctx c = {xs, f, tmp};
    if (pure) parallel_for_grain(n, fine_grain(n), each_range, &c);
    else each_range(&c, 0, n);
    mim_val *r = mim_array_lit(n, tmp);
    for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
    free(tmp);
    return r;
}

/* ==== indexing =========================================================== */

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
    if (__atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) > 1 || newdt != xs->dtype) {
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

/* ==== FFI marshalling ==================================================== */

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

/* The interaction-net layer lives in its own file but is compiled as part of
 * this translation unit so it can use the allocator and kernels directly. */
#ifndef MIMIC_NO_NET
#include "mimic_net.c"
#endif
