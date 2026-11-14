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
 *   MIMIC_NO_EACH_LOOP  `¨` over waiting functions uses the cores, not a
 *                       task per element on the event loop (for debugging)
 */
#include <math.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
#include <time.h>

#if defined(__wasi__) || defined(MIMIC_NO_THREADS)
#define MIM_THREADS 0
#elif defined(_WIN32)
#define MIM_THREADS 1
#include <winsock2.h> /* before windows.h, which would bring the old winsock.h */
#include <ws2tcpip.h>
#include <windows.h>
#include <bcrypt.h>
/* what the C library on Windows lacks */
#define gmtime_r(t, tm) (gmtime_s((tm), (t)) == 0 ? (tm) : NULL)
#define localtime_r(t, tm) (localtime_s((tm), (t)) == 0 ? (tm) : NULL)
static void *memmem(const void *h, size_t hn, const void *n, size_t nn) {
    if (nn == 0) return (void *)h;
    for (const char *p = (const char *)h; hn >= nn; p++, hn--)
        if (*p == *(const char *)n && memcmp(p, n, nn) == 0) return (void *)p;
    return NULL;
}
#include <pthread.h> /* winpthreads (mingw-w64): futures, the net reducer */
#include <unistd.h>
#else
#define MIM_THREADS 1
#include <pthread.h>
#include <sched.h>
#include <unistd.h>
#endif

/* Platform BLAS, looked up at run time (no link-time dependency): Apple's
 * Accelerate everywhere on Apple systems, OpenBLAS when installed on Linux. */
#if (defined(__APPLE__) || defined(__linux__)) && !defined(__wasi__)
#define MIM_DLBLAS 1
#include <dlfcn.h>
#endif

/* BOXED: a vector of handles (nested arrays, lists of strings/records).
 * OBJ: a record or union variant — `len` field handles plus a descriptor. */
typedef enum { MIM_I64 = 0, MIM_F64 = 1, MIM_BOOL = 2, MIM_CHAR = 3, MIM_BOXED = 4, MIM_OBJ = 5, MIM_FN = 6, MIM_MAP = 7, MIM_U8 = 8, MIM_F32 = 9, MIM_I32 = 10 } mim_dtype;
/* floats of either width (f32 arithmetic is computed in double and rounded
 * to f32 after every operation: exactly f32 arithmetic for + - × ÷) */
#define IS_FLOAT(dt) ((dt) == MIM_F64 || (dt) == MIM_F32)
/* plain numbers (and truth values): i64, f64, bool, f32, i32, u8 */
#define IS_NUMDT(dt) ((dt) == MIM_I64 || (dt) == MIM_F64 || (dt) == MIM_BOOL || (dt) == MIM_F32 || (dt) == MIM_I32 || (dt) == MIM_U8)

/* Emitted by the compiler, one per record type / union variant. */
typedef struct {
    const char *type_name;
    const char *variant; /* NULL for records */
    int32_t tag;         /* variant index */
    int32_t nfields;
    const char *const *fields; /* record field names (NULL for variants) */
    /* fields stored as raw 8-byte numbers instead of handles (bit i: field
     * i): i64, f64 (fbits) or bool (bbits, 0/1); the program's own types */
    uint64_t raw, fbits, bbits;
} mim_desc;

/* The standard library's type descriptors, defined by each compiled program.
 * On Windows they are weak: a program that never uses one (a raw `.mnet`)
 * links without it, as unused code is dropped before linking elsewhere. */
#ifdef _WIN32
#define MIM_STD_DESC extern const mim_desc __attribute__((weak))
#else
#define MIM_STD_DESC extern const mim_desc
#endif

/* FN: a function value — `len` captured handles plus a descriptor (stored in
 * `desc`); `code(env, args)` borrows both and returns an owned value. */
typedef struct mim_val mim_val;
typedef mim_val *(*mim_code)(mim_val *const *env, mim_val *const *args);
typedef struct {
    const char *name;
    mim_code code;
    int32_t arity;
    int32_t pure; /* 0 effects in order, 1 pure, 2 effects that may overlap */
} mim_fn_desc;

typedef struct mim_val {
    int64_t rc; /* atomic; >= MIM_IMMORTAL means never freed */
    int32_t dtype;
    int32_t rank;
    int64_t len;
    const mim_desc *desc; /* MIM_OBJ only */
    /* the elements, right after the header (one allocation per value) */
    _Alignas(8) unsigned char data[];
} mim_val;

#define IS_HANDLES(v) ((v)->dtype == MIM_BOXED || (v)->dtype == MIM_OBJ || (v)->dtype == MIM_FN || (v)->dtype == MIM_MAP)
#define FN_DESC(v) ((const mim_fn_desc *)(const void *)(v)->desc)

#define MIM_IMMORTAL ((int64_t)1 << 62)

typedef mim_val *(*mim_fn1)(mim_val *);

enum { OP_ADD, OP_SUB, OP_MUL, OP_DIV, OP_MOD, OP_EQ, OP_NE, OP_LT, OP_LE, OP_GT, OP_GE, OP_AND, OP_OR, OP_IDIV, OP_MAX2, OP_MIN2,
       OP_BAND, OP_BOR, OP_BXOR, OP_SHL, OP_SHR, OP_USHR };
#define IS_BITOP(op) ((op) >= OP_BAND && (op) <= OP_USHR)
/* shift amounts use their low 6 bits, as the hardware does */
#define SHL64(x, y) ((int64_t)((uint64_t)(x) << ((y) & 63)))
#define SHR64(x, y) ((x) >> ((y) & 63))
#define USHR64(x, y) ((int64_t)((uint64_t)(x) >> ((y) & 63)))
enum { UOP_NEG, UOP_NOT, UOP_ABS, UOP_BNOT };
enum { P_IOTA, P_SHAPE, P_REVERSE, P_SUM, P_PRODUCT, P_MAX, P_MIN, P_LEN, P_PRINT, P_TAKE, P_DROP, P_KEEP, P_EACH, P_RANGE,
       P_RESHAPE = 21, P_TRANSPOSE = 22, P_MATMUL = 23 };

void mim_panic(const char *msg);

/* floored modulo with a single division */
static inline int64_t floormod(int64_t x, int64_t y) {
    int64_t r = x % y;
    return (r != 0 && ((r ^ y) < 0)) ? r + y : r;
}

/* floored integer division (matches `%`): 7 div 2 = 3, -7 div 2 = -4 */
static int64_t floordiv(int64_t x, int64_t y) {
    if (y == 0) mim_panic("integer division by zero");
    int64_t q = x / y;
    if ((x % y != 0) && ((x < 0) != (y < 0))) q--;
    return q;
}
enum { TY_INT = 1, TY_FLOAT, TY_BOOL, TY_CHAR, TY_STR, TY_INTVEC, TY_FLOATVEC, TY_BOOLVEC, TY_FN, TY_BYTES, TY_F32, TY_I32, TY_F32VEC, TY_I32VEC };

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
/* the same, for compiled kernels to read without a call */
int64_t mim_grain = 16384;
static int g_nthreads = 1;
/* g_nthreads for compiled code (a scan on one thread makes one pass) */
int64_t mim_threads = 1;
static _Thread_local int t_in_parallel = 0;
extern int32_t mim_fj_open;

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
        __atomic_store_n(&mim_fj_open, 0, __ATOMIC_RELAXED); /* no regions inside */
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
        __atomic_store_n(&mim_fj_open, 1, __ATOMIC_RELAXED);
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

static void parallel_for(int64_t n, mim_task fn, void *ctx);

/* ==== compiler-generated (fused) kernels ===================================
 * The compiler fuses chains of elementwise array operations into one loop and
 * runs it with the same fixed-size chunks as the built-in kernels, so results
 * (including floating-point reductions) do not depend on the thread count. */
void mim_par_for(int64_t n, void (*f)(void *, int64_t, int64_t), void *ctx) { parallel_for(n, f, ctx); }
void mim_par_for_grain(int64_t n, int64_t grain, void (*f)(void *, int64_t, int64_t), void *ctx) { parallel_for_grain(n, grain, f, ctx); }

/* A compiled loop whose elements may each be much work (they call into
 * recursion): time the first element, then share out the rest (in small
 * chunks: one element each when there are few) when that is worth waking
 * the threads, as mim_each_fn does. */
typedef struct {
    void (*f)(void *, int64_t, int64_t);
    void *ctx;
} timed_ctx;

static void timed_range(void *c, int64_t lo, int64_t hi) {
    timed_ctx *t = (timed_ctx *)c;
    t->f(t->ctx, lo + 1, hi + 1);
}

void mim_par_for_timed(int64_t n, void (*f)(void *, int64_t, int64_t), void *ctx) {
    if (n <= 0) return;
    if (n == 1 || g_nthreads <= 1 || t_in_parallel) {
        f(ctx, 0, n);
        return;
    }
    /* many elements: the usual chunks (one cheap first element says little
     * about the rest, and there is work enough to share either way) */
    if (n > mim_grain) {
        parallel_for(n, f, ctx);
        return;
    }
    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    f(ctx, 0, 1);
    clock_gettime(CLOCK_MONOTONIC, &b);
    int64_t one = (int64_t)(b.tv_sec - a.tv_sec) * 1000000000 + (b.tv_nsec - a.tv_nsec);
    if (one * (n - 1) > 50000) {
        timed_ctx c = {f, ctx};
        parallel_for_grain(n - 1, fine_grain(n - 1), timed_range, &c);
    } else {
        f(ctx, 1, n);
    }
}
int64_t mim_par_grain(void);

/* ==== fork-join ===========================================================
 * Independent calls in one expression (`f(n - 1) + f(n - 2)`,
 * `Node(make(d - 1), make(d - 1))`) run in parallel. The compiler emits
 * three versions of such a function: the plain one, which may open a
 * fork-join region at its first such site (mim_fj_enter); a fork version,
 * used inside the region, which hands all but the last call of a site to
 * this thread's deque (mim_fj_fork) while it runs the last itself, then
 * takes back or waits for each (mim_fj_join); and a sequential version, used
 * below the depth `mim_fj_cut` and outside regions.
 *
 * The region borrows the thread pool as one job: every worker steals tasks
 * from the others' deques until the region closes. Inside a region,
 * data-parallel loops run sequentially (t_in_parallel), as in any task. */

/* A hint for compiled code: may a region open now? (more threads than one
 * and none in use; mim_fj_enter decides) */
int32_t mim_fj_open = 0;
/* The fork depth below which a fork version still forks (2^cut tasks). */
int32_t mim_fj_cut = 0;

typedef struct mim_fj_task {
    void (*fn)(void *);
    void *ctx;
    int32_t state; /* 0 queued, 1 taken, 2 done */
} mim_fj_task;
_Static_assert(sizeof(mim_fj_task) <= 32, "the compiler reserves 32 bytes per task");

#define FJ_CAP 1024
typedef struct {
    int32_t lock;
    int32_t top, bot;
    mim_fj_task *items[FJ_CAP];
    char pad[64];
} fj_deque;

static fj_deque *g_fjq = NULL;
static int32_t g_fj_done = 0;
static _Thread_local fj_deque *t_fj_q = NULL;
static _Thread_local int t_fj_nest = 0;
/* tiny regions are not worth opening: after one, skip the next few */
static _Thread_local int t_fj_skip = 0, t_fj_backoff = 0;

static inline void fj_relax(void) {
#if defined(__aarch64__)
    __asm__ __volatile__("yield");
#elif defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#endif
}

/* Waiting for work, politely: spin a little (a new task is taken at
 * once), then give the core away (other threads, other programs), then
 * sleep in short naps. `misses` counts the checks that found nothing. */
static void fj_idle(int misses) {
    if (misses < 64) {
        fj_relax();
    } else if (misses < 2048) {
#if MIM_THREADS && defined(_WIN32)
        SwitchToThread();
#elif MIM_THREADS
        sched_yield();
#endif
    } else {
#if MIM_THREADS && defined(_WIN32)
        /* a real nap, as nanosleep below: Sleep(0) returns at once when no
         * other thread waits, so idle workers spun on the core (or the
         * hyper-thread) the working ones need; Sleep(1) sleeps 1-15 ms */
        static _Thread_local HANDLE nap;
        if (!nap) nap = CreateWaitableTimerExW(NULL, NULL, 0x2 /* CREATE_WAITABLE_TIMER_HIGH_RESOLUTION */, TIMER_ALL_ACCESS);
        LARGE_INTEGER due;
        due.QuadPart = -200; /* 20 µs, in 100 ns units, relative */
        if (nap && SetWaitableTimer(nap, &due, 0, NULL, NULL, FALSE)) WaitForSingleObject(nap, INFINITE);
        else SwitchToThread();
#elif MIM_THREADS
        struct timespec ts = {0, 20000}; /* 20 µs */
        nanosleep(&ts, NULL);
#endif
    }
}

static inline void fj_lock(fj_deque *q) {
    while (__atomic_exchange_n(&q->lock, 1, __ATOMIC_ACQUIRE)) fj_relax();
}
static inline void fj_unlock(fj_deque *q) { __atomic_store_n(&q->lock, 0, __ATOMIC_RELEASE); }

static void fj_run(mim_fj_task *t) {
    t->fn(t->ctx);
    __atomic_store_n(&t->state, 2, __ATOMIC_RELEASE);
}

/* The oldest task of another thread's deque, if any. */
static mim_fj_task *fj_steal(fj_deque *self) {
    int n = g_nthreads;
    int start = (int)(((uintptr_t)self >> 6) % (uintptr_t)n);
    for (int k = 0; k < n; k++) {
        fj_deque *q = &g_fjq[(start + k) % n];
        if (q == self) continue;
        if (__atomic_load_n(&q->top, __ATOMIC_RELAXED) >= __atomic_load_n(&q->bot, __ATOMIC_RELAXED)) continue;
        fj_lock(q);
        mim_fj_task *t = NULL;
        if (q->top < q->bot) {
            t = q->items[q->top++];
            t->state = 1;
            if (q->top == q->bot) q->top = q->bot = 0;
        }
        fj_unlock(q);
        if (t) return t;
    }
    return NULL;
}

void mim_fj_fork(mim_fj_task *t, void (*fn)(void *), void *ctx) {
    t->fn = fn;
    t->ctx = ctx;
    t->state = 0;
    fj_deque *q = t_fj_q;
    if (q) {
        fj_lock(q);
        if (q->bot < FJ_CAP) {
            q->items[q->bot++] = t;
            fj_unlock(q);
            return;
        }
        fj_unlock(q);
    }
    fj_run(t); /* no region, or a full deque: run it now */
}

void mim_fj_join(mim_fj_task *t) {
    if (__atomic_load_n(&t->state, __ATOMIC_ACQUIRE) == 2) return;
    fj_deque *q = t_fj_q;
    if (q) {
        fj_lock(q);
        if (q->bot > q->top && q->items[q->bot - 1] == t) {
            q->bot--;
            if (q->top == q->bot) q->top = q->bot = 0;
            fj_unlock(q);
            fj_run(t);
            return;
        }
        fj_unlock(q);
    }
    /* stolen: help with other work until it is done (not too deep: each
     * task helped with runs on top of this stack) */
    int misses = 0;
    while (__atomic_load_n(&t->state, __ATOMIC_ACQUIRE) != 2) {
        mim_fj_task *o = (q && t_fj_nest < 8) ? fj_steal(q) : NULL;
        if (o) {
            t_fj_nest++;
            fj_run(o);
            t_fj_nest--;
            misses = 0;
        } else {
            fj_idle(misses++);
        }
    }
}

#if MIM_THREADS
/* One pool worker's part of a region: steal until it closes. */
static void fj_worker(void *ctx, int64_t lo, int64_t hi) {
    (void)ctx;
    (void)hi;
    fj_deque *self = &g_fjq[lo + 1];
    t_fj_q = self;
    int misses = 0;
    while (!__atomic_load_n(&g_fj_done, __ATOMIC_ACQUIRE)) {
        mim_fj_task *t = fj_steal(self);
        if (t) {
            fj_run(t);
            misses = 0;
        } else {
            fj_idle(misses++);
        }
    }
    t_fj_q = NULL;
}

static mim_job g_fj_job;
static struct timespec g_fj_start;
#endif

/* Open a region on this thread: 1 when it did (close it with mim_fj_leave). */
int32_t mim_fj_enter(void) {
#if MIM_THREADS
    if (g_nthreads <= 1 || t_in_parallel || t_fj_q) return 0;
    if (t_fj_skip > 0) {
        t_fj_skip--;
        return 0;
    }
    if (!g_started) start_pool();
    if (!g_fjq) {
        fj_deque *q = (fj_deque *)calloc((size_t)g_nthreads, sizeof(fj_deque));
        if (!q) return 0;
        g_fjq = q;
    }
    mutex_lock(&g_mu);
    if (g_job != NULL) { /* the pool is busy (another thread's loop) */
        mutex_unlock(&g_mu);
        return 0;
    }
    __atomic_store_n(&mim_fj_open, 0, __ATOMIC_RELAXED);
    __atomic_store_n(&g_fj_done, 0, __ATOMIC_RELAXED);
    int64_t w = g_nthreads - 1;
    mim_job j = {fj_worker, NULL, w, w, 1, 0, w, 0};
    g_fj_job = j;
    g_job = &g_fj_job;
    g_gen++;
    cond_broadcast(&g_work);
    mutex_unlock(&g_mu);
    t_in_parallel = 1;
    t_fj_q = &g_fjq[0];
    clock_gettime(CLOCK_MONOTONIC, &g_fj_start);
    return 1;
#else
    return 0;
#endif
}

void mim_fj_leave(void) {
#if MIM_THREADS
    __atomic_store_n(&g_fj_done, 1, __ATOMIC_RELEASE);
    run_chunks(&g_fj_job); /* workers that never came: their parts end at once */
    mutex_lock(&g_mu);
    while (__atomic_load_n(&g_fj_job.remaining, __ATOMIC_ACQUIRE) > 0) cond_wait(&g_done, &g_mu);
    g_job = NULL;
    while (g_fj_job.active > 0) cond_wait(&g_done, &g_mu);
    mutex_unlock(&g_mu);
    t_fj_q = NULL;
    t_in_parallel = 0;
    __atomic_store_n(&mim_fj_open, 1, __ATOMIC_RELAXED);
    struct timespec b;
    clock_gettime(CLOCK_MONOTONIC, &b);
    int64_t ns = (int64_t)(b.tv_sec - g_fj_start.tv_sec) * 1000000000 + (b.tv_nsec - g_fj_start.tv_nsec);
    if (ns < 200000) {
        t_fj_backoff = t_fj_backoff ? (t_fj_backoff < 4096 ? t_fj_backoff * 2 : 4096) : 1;
        t_fj_skip = t_fj_backoff;
    } else {
        t_fj_backoff = 0;
    }
#endif
}

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
    case MIM_F32:
    case MIM_I32: return 4;
    case MIM_BOXED:
    case MIM_OBJ:
    case MIM_FN:
    case MIM_MAP: return sizeof(void *);
    default: return 1;
    }
}

#define SMALL_BYTES 16 /* payload bytes (incl. NUL) served by the free list */
#define MID_BYTES 64   /* a second free list: short vectors, small records, short strings */
#define FREE_LIST_CAP (1 << 18) /* blocks per size class and thread; also in the compiler (release_fn) */
#define MAX_SLOTS 257

/* per-thread counters, padded to 128-byte lines (Apple Silicon) */
typedef struct { int64_t n; int64_t nodes; int64_t itrs; char pad[104]; } padded_count;
static padded_count g_live_slots[MAX_SLOTS];
static int g_next_slot = 0;
/* Everything the allocator keeps per thread, in one place: one thread-local
 * lookup per allocation (each lookup is a call on some systems). Blocks are
 * kept in free lists by size class: payloads up to 16, 64 and 256 bytes. */
#define SIZE_CLASSES 3
static const size_t class_bytes[SIZE_CLASSES] = {SMALL_BYTES, MID_BYTES, 256};

typedef struct {
    mim_val *free[SIZE_CLASSES];
    int free_len[SIZE_CLASSES];
    padded_count *live;        /* this thread's live-value counter */
} heap_state;

static _Thread_local heap_state t_heap;

static padded_count *live_of(heap_state *h) {
    if (!h->live) {
        int s = __atomic_fetch_add(&g_next_slot, 1, __ATOMIC_RELAXED);
        h->live = &g_live_slots[s < MAX_SLOTS ? s : MAX_SLOTS - 1];
    }
    return h->live;
}

static padded_count *live_slot(void) { return live_of(&t_heap); }

/* This thread's heap, for compiled code: a function fetches it once and
 * then allocates and frees short vectors inline (codegen: alloc_vec_fn and
 * release_fn mirror alloc and dealloc below for that case). A coroutine
 * never moves between threads, so the pointer stays right for the whole
 * call. The layout is shared with the compiler: keep them in step. */
_Static_assert(SIZE_CLASSES == 3, "the compiler's inline allocator knows three size classes");
_Static_assert(offsetof(heap_state, free_len) == 3 * sizeof(void *), "heap_state layout is shared with the compiler");
_Static_assert(offsetof(padded_count, n) == 0, "padded_count layout is shared with the compiler");
heap_state *mim_heap(void) {
    heap_state *h = &t_heap;
    live_of(h);
    return h;
}

/* The size class of a payload, or SIZE_CLASSES when it is bigger. */
static inline int size_class(size_t bytes) {
    return bytes <= SMALL_BYTES ? 0 : bytes <= MID_BYTES ? 1 : bytes <= 256 ? 2 : SIZE_CLASSES;
}

/* Payload bytes: vectors of 1-byte elements (text, bytes, truth values) get
 * one more, kept 0, so text is NUL-terminated for C. The compiler's inline
 * allocator (alloc_vec_fn, release_fn) computes the same. */
static inline size_t payload_bytes(int dtype, int64_t len) {
    size_t es = elsize(dtype);
    return (size_t)len * es + (es == 1);
}

/* Small blocks are carved from chunks and never go back to malloc: a free
 * list that grows past FREE_LIST_CAP hands the whole list to a shared pool,
 * and an empty one takes a batch from there before it carves a new chunk.
 * (Threads never contend in malloc for small values.) */
#define CHUNK_BYTES (256 * 1024)
#define POOL_BATCH 4096
typedef struct {
    int32_t lock;
    int64_t n;
    mim_val *head;
    char pad[104];
} block_pool;
static block_pool g_pool[SIZE_CLASSES];

static inline void pool_lock(block_pool *p) {
    while (__atomic_exchange_n(&p->lock, 1, __ATOMIC_ACQUIRE)) {
    }
}
static inline void pool_unlock(block_pool *p) { __atomic_store_n(&p->lock, 0, __ATOMIC_RELEASE); }

/* Hand this thread's list of class c to the pool. */
static void pool_give(heap_state *h, int c) {
    mim_val *head = h->free[c];
    if (!head) return;
    mim_val *tail = head;
    while (*(mim_val **)tail) tail = *(mim_val **)tail;
    block_pool *p = &g_pool[c];
    pool_lock(p);
    *(mim_val **)tail = p->head;
    p->head = head;
    p->n += h->free_len[c];
    pool_unlock(p);
    h->free[c] = NULL;
    h->free_len[c] = 0;
}

/* Fill this thread's empty list of class c: a batch from the pool, or a
 * new chunk. */
static void refill(heap_state *h, int c) {
    block_pool *p = &g_pool[c];
    if (__atomic_load_n(&p->n, __ATOMIC_RELAXED) > 0) {
        pool_lock(p);
        mim_val *head = p->head, *tail = NULL, *v = head;
        int k = 0;
        while (v && k < POOL_BATCH) {
            tail = v;
            v = *(mim_val **)v;
            k++;
        }
        if (tail) {
            p->head = v;
            p->n -= k;
            *(mim_val **)tail = h->free[c];
            h->free[c] = head;
            h->free_len[c] += k;
        }
        pool_unlock(p);
        if (k) return;
    }
    size_t block = sizeof(mim_val) + class_bytes[c];
    size_t nb = CHUNK_BYTES / block;
    char *chunk = (char *)malloc(nb * block);
    if (!chunk) mim_panic("out of memory");
    for (size_t i = nb; i-- > 0;) {
        mim_val *b = (mim_val *)(chunk + i * block);
        *(mim_val **)b = h->free[c];
        h->free[c] = b;
    }
    h->free_len[c] += (int)nb;
}

static mim_val *alloc(int dtype, int rank, int64_t len) {
    size_t bytes = payload_bytes(dtype, len);
    heap_state *h = &t_heap;
    int c = size_class(bytes);
    mim_val *v;
    if (c < SIZE_CLASSES) {
        if (!h->free[c]) refill(h, c);
        v = h->free[c];
        h->free[c] = *(mim_val **)v;
        h->free_len[c]--;
    } else {
        v = (mim_val *)malloc(sizeof(mim_val) + bytes);
        if (!v) mim_panic("out of memory");
    }
    v->rc = 1;
    v->dtype = dtype;
    v->rank = rank;
    v->len = len;
    v->desc = NULL;
    if (elsize(dtype) == 1) ((char *)v->data)[bytes - 1] = 0;
    padded_count *lc = live_of(h);
    __atomic_store_n(&lc->n, lc->n + 1, __ATOMIC_RELAXED);
    return v;
}

void mim_release(mim_val *v);

static void map_dealloc_meta(mim_val *m);

static void dealloc_in(heap_state *h, mim_val *v);

/* Drop one reference (h: this thread's heap). The only reference: no other
 * thread can see the value, so no atomic update. */
static inline void release_in(heap_state *h, mim_val *v) {
    if (!v) return;
    int64_t rc = __atomic_load_n(&v->rc, __ATOMIC_ACQUIRE);
    if (rc == 1) dealloc_in(h, v);
    else if (rc < MIM_IMMORTAL && __atomic_sub_fetch(&v->rc, 1, __ATOMIC_ACQ_REL) == 0) dealloc_in(h, v);
}

static inline int holds_values(int dt) { return dt == MIM_BOXED || dt == MIM_OBJ || dt == MIM_FN || dt == MIM_MAP; }

/* Drop a reference: 1 when it was the last. */
static inline int drop_ref(mim_val *v) {
    if (!v) return 0;
    int64_t rc = __atomic_load_n(&v->rc, __ATOMIC_ACQUIRE);
    return rc == 1 || (rc < MIM_IMMORTAL && __atomic_sub_fetch(&v->rc, 1, __ATOMIC_ACQ_REL) == 0);
}

/* Give one value's block back (what it holds is released already). */
static void free_block(heap_state *h, mim_val *v) {
    int dt = v->dtype;
    if (dt == MIM_MAP) map_dealloc_meta(v);
    if (v->rank >= 2 && dt != MIM_MAP) free((void *)v->desc); /* the shape of a matrix */
    size_t bytes = payload_bytes(dt, v->len);
    padded_count *lc = live_of(h);
    __atomic_store_n(&lc->n, lc->n - 1, __ATOMIC_RELAXED);
    int c = size_class(bytes);
    if (c < SIZE_CLASSES) {
        if (h->free_len[c] >= FREE_LIST_CAP) pool_give(h, c);
        *(mim_val **)v = h->free[c];
        h->free[c] = v;
        h->free_len[c]++;
    } else {
        free(v);
    }
}

static void release_all(mim_val **items, int64_t n);

typedef struct {
    mim_fj_task task;
    int64_t n;
    mim_val *items[];
} release_job;

static void release_task(void *c) {
    release_job *j = (release_job *)c;
    release_all(j->items, j->n);
}

/* Free values whose last reference is gone, and everything only they held:
 * depth-first with a stack of its own (any depth of nesting). A big release
 * (a large tree) shares its pending subtrees out to the fork-join pool. */
static void release_all(mim_val **items, int64_t n) {
    heap_state *h = &t_heap;
    mim_val *local[64];
    mim_val **st = local;
    int64_t cap = 64, top = 0;
    release_job *forked[32];
    int nf = 0, opened = 0;
    int64_t done = 0;
    for (int64_t i = 0; i < n; i++) {
        if (top == cap) {
            mim_val **ns = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap * 2);
            if (!ns) mim_panic("out of memory");
            memcpy(ns, st, sizeof(mim_val *) * (size_t)top);
            if (st != local) free(st);
            st = ns;
            cap *= 2;
        }
        st[top++] = items[i];
    }
    while (top > 0) {
        mim_val *v = st[--top];
        mim_val **hs = (mim_val **)v->data;
        uint64_t raw = v->dtype == MIM_OBJ && v->desc ? v->desc->raw : 0;
        for (int64_t i = 0; i < v->len; i++) {
            if (i < 64 && (raw >> i) & 1) continue; /* a number, not a handle */
            mim_val *c = hs[i];
            if (!drop_ref(c)) continue;
            if (!holds_values(c->dtype)) {
                free_block(h, c);
                continue;
            }
            if (top == cap) {
                mim_val **ns = (mim_val **)malloc(sizeof(mim_val *) * (size_t)cap * 2);
                if (!ns) mim_panic("out of memory");
                memcpy(ns, st, sizeof(mim_val *) * (size_t)top);
                if (st != local) free(st);
                st = ns;
                cap *= 2;
            }
            st[top++] = c;
        }
        free_block(h, v);
        /* big: the older half of the stack (the larger subtrees) to the pool */
        if ((++done & 0xFFFF) == 0 && top >= 2 && nf < 32) {
            if (!t_fj_q && !opened && __atomic_load_n(&mim_fj_open, __ATOMIC_RELAXED) && mim_fj_enter()) opened = 1;
            if (t_fj_q) {
                int64_t k = top / 2;
                release_job *j = (release_job *)malloc(sizeof(release_job) + sizeof(mim_val *) * (size_t)k);
                if (j) {
                    j->n = k;
                    memcpy(j->items, st, sizeof(mim_val *) * (size_t)k);
                    memmove(st, st + k, sizeof(mim_val *) * (size_t)(top - k));
                    top -= k;
                    forked[nf++] = j;
                    mim_fj_fork(&j->task, release_task, j);
                }
            }
        }
    }
    while (nf > 0) {
        release_job *j = forked[--nf];
        mim_fj_join(&j->task);
        free(j);
    }
    if (opened) mim_fj_leave();
    if (st != local) free(st);
}

static void dealloc_in(heap_state *h, mim_val *v) {
    if (holds_values(v->dtype)) release_all(&v, 1);
    else free_block(h, v);
}

static void dealloc(mim_val *v) { dealloc_in(&t_heap, v); }

/* Give this thread's cached blocks to the shared pool (the thread ends). */
static void free_list_drain(void) {
    heap_state *h = &t_heap;
    for (int c = 0; c < SIZE_CLASSES; c++) pool_give(h, c);
}

/* A fresh vector for a fused kernel's output. */
mim_val *mim_alloc_vec(int32_t dtype, int64_t n) { return alloc(dtype, 1, n); }

void mim_retain(mim_val *v) {
    if (v && __atomic_load_n(&v->rc, __ATOMIC_RELAXED) < MIM_IMMORTAL) __atomic_fetch_add(&v->rc, 1, __ATOMIC_RELAXED);
}

void mim_release(mim_val *v) {
    if (!v) return;
    int64_t rc = __atomic_load_n(&v->rc, __ATOMIC_ACQUIRE);
    if (rc == 1 || (rc < MIM_IMMORTAL && __atomic_sub_fetch(&v->rc, 1, __ATOMIC_ACQ_REL) == 0)) dealloc_in(&t_heap, v);
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

static int g_rt_state = 0; /* 0 = not started, 1 = starting, 2 = ready */

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <shellapi.h>
/* Windows: bytes out as they are (no \n → \r\n), UTF-8 on the console, and
 * the arguments as UTF-8 (the C runtime's argv uses the local code page). */
static void win_start(int32_t *argc, char ***argv) {
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
    _setmode(_fileno(stdin), _O_BINARY);
    SetConsoleOutputCP(CP_UTF8);
    /* ANSI escape codes (colours, cursor moves) on the console, as on Unix */
    HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD mode;
    if (out != INVALID_HANDLE_VALUE && GetConsoleMode(out, &mode)) SetConsoleMode(out, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    if (*argc <= 0) return;
    int n = 0;
    wchar_t **w = CommandLineToArgvW(GetCommandLineW(), &n);
    if (!w || n <= 0) return;
    char **a = (char **)calloc((size_t)n + 1, sizeof(char *));
    for (int i = 0; i < n; i++) {
        int len = WideCharToMultiByte(CP_UTF8, 0, w[i], -1, NULL, 0, NULL, NULL);
        a[i] = (char *)malloc(len > 0 ? (size_t)len : 1);
        if (len > 0) WideCharToMultiByte(CP_UTF8, 0, w[i], -1, a[i], len, NULL, NULL);
        else a[i][0] = 0;
    }
    LocalFree(w);
    *argc = n;
    *argv = a;
}
#endif

#if !defined(_WIN32) && !defined(__wasi__)
#include <sys/resource.h>
/* As many open files as the system allows this process (macOS starts at
 * 256): a server or a client with thousands of connections needs them. */
static void raise_open_files(void) {
    struct rlimit r;
    if (getrlimit(RLIMIT_NOFILE, &r) != 0 || r.rlim_cur >= r.rlim_max) return;
    rlim_t want = r.rlim_max;
#ifdef __APPLE__
    if (want > 24576 || want == RLIM_INFINITY) want = 24576; /* OPEN_MAX rules on macOS */
#endif
    for (; want > r.rlim_cur; want /= 2) {
        struct rlimit n = {want, r.rlim_max};
        if (setrlimit(RLIMIT_NOFILE, &n) == 0) return;
    }
}
#endif

void mim_rt_init(int32_t argc, char **argv) {
    __atomic_store_n(&g_rt_state, 2, __ATOMIC_RELEASE);
#if !defined(_WIN32) && !defined(__wasi__)
    raise_open_files();
#endif
#ifdef _WIN32
    win_start(&argc, &argv);
#endif
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
        mim_grain = g_grain;
        g_grain_forced = 1;
    }
#if MIM_THREADS
    if (t && atoi(t) > 0) {
        g_nthreads = atoi(t);
    } else {
#ifdef _WIN32
        /* every processor group (GetSystemInfo stops at 64) */
        DWORD n = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS);
        g_nthreads = n > 0 ? (int)n : 1;
#else
        long n = sysconf(_SC_NPROCESSORS_ONLN);
        g_nthreads = n > 0 ? (int)n : 1;
#endif
    }
    if (g_nthreads > 256) g_nthreads = 256;
    mim_threads = g_nthreads;
    if (g_nthreads > 1) {
        int lg = 0;
        while ((1 << lg) < g_nthreads) lg++;
        mim_fj_cut = lg + 6; /* ~64 tasks per thread */
        mim_fj_open = 1;
    }
#else
    (void)t;
    g_nthreads = 1;
#endif
}

/* Functions exported to C (`extern "C" fn`) may be the first Mimic code to
 * run, from a C program's main: start the runtime once, from any thread. */
void mim_rt_ensure(void) {
    if (__atomic_load_n(&g_rt_state, __ATOMIC_ACQUIRE) == 2) return;
    int expected = 0;
    if (__atomic_compare_exchange_n(&g_rt_state, &expected, 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        mim_rt_init(0, NULL);
        return;
    }
    while (__atomic_load_n(&g_rt_state, __ATOMIC_ACQUIRE) != 2) {
    }
}

const char *mim_cstr(mim_val *v);

/* A string returned to C: a malloc'd copy the caller frees with free(). */
char *mim_cstr_dup(mim_val *v) {
    const char *s = mim_cstr(v);
    size_t n = strlen(s);
    char *d = malloc(n + 1);
    if (d) memcpy(d, s, n + 1);
    return d;
}

/* tasks started with ui::detach that are still running (nobody waits for
 * them): what they hold is alive at exit by design */
static int64_t g_detached_running = 0;

static void memo_release_all(void);

int32_t mim_rt_shutdown(int32_t leak_check) {
    fflush(stdout);
    memo_release_all(); /* rows kept for the next frame (mim_each_memo) */
    if (__atomic_load_n(&g_detached_running, __ATOMIC_ACQUIRE) > 0) leak_check = 0;
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
    case MIM_F32: return (double)((float *)v->data)[i];
    case MIM_I32: return (double)((int32_t *)v->data)[i];
    default: return (double)((uint8_t *)v->data)[i];
    }
}

static int64_t geti(const mim_val *v, int64_t i) {
    switch (v->dtype) {
    case MIM_F64: return (int64_t)((double *)v->data)[i];
    case MIM_I64: return ((int64_t *)v->data)[i];
    case MIM_F32: return (int64_t)((float *)v->data)[i];
    case MIM_I32: return ((int32_t *)v->data)[i];
    default: return (int64_t)((uint8_t *)v->data)[i];
    }
}

static void setf(mim_val *v, int64_t i, double x) {
    switch (v->dtype) {
    case MIM_F64: ((double *)v->data)[i] = x; break;
    case MIM_I64: ((int64_t *)v->data)[i] = (int64_t)x; break;
    case MIM_F32: ((float *)v->data)[i] = (float)x; break;
    case MIM_I32: ((int32_t *)v->data)[i] = (int32_t)(uint32_t)(int64_t)x; break;
    default: ((uint8_t *)v->data)[i] = (uint8_t)x; break;
    }
}

static void seti(mim_val *v, int64_t i, int64_t x) {
    switch (v->dtype) {
    case MIM_F64: ((double *)v->data)[i] = (double)x; break;
    case MIM_I64: ((int64_t *)v->data)[i] = x; break;
    case MIM_F32: ((float *)v->data)[i] = (float)x; break;
    case MIM_I32: ((int32_t *)v->data)[i] = (int32_t)(uint32_t)x; break; /* wraps, as i32 arithmetic does */
    case MIM_BOOL: ((uint8_t *)v->data)[i] = x != 0; break;
    default: ((uint8_t *)v->data)[i] = (uint8_t)x; break;
    }
}

static const char *dtname(const mim_val *v) {
    static const char *s[] = {"i64", "f64", "bool", "char"};
    static const char *vs[] = {"[i64]", "[f64]", "[bool]", "str"};
    if (!v) return "()";
    if (v->dtype == MIM_OBJ) return v->desc && v->desc->type_name ? v->desc->type_name : "record";
    if (v->dtype == MIM_BOXED) return "list";
    if (v->dtype == MIM_FN) return "function";
    if (v->dtype == MIM_MAP) return "map";
    if (v->dtype == MIM_U8) return v->rank == 0 ? "i64" : "[u8]";
    if (v->dtype == MIM_F32) return v->rank == 0 ? "f32" : "[f32]";
    if (v->dtype == MIM_I32) return v->rank == 0 ? "i32" : "[i32]";
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

mim_val *mim_float32(double x) {
    mim_val *v = alloc(MIM_F32, 0, 1);
    ((float *)v->data)[0] = (float)x;
    return v;
}

mim_val *mim_int32(int64_t x) {
    mim_val *v = alloc(MIM_I32, 0, 1);
    ((int32_t *)v->data)[0] = (int32_t)(uint32_t)x;
    return v;
}

/* The element type of an arithmetic result. Floats beat integers; between
 * two widths of the same kind, a list keeps its own type against a single
 * value (`[f32] * 0.5` is `[f32]`), else the wider wins. The type checker
 * (types.rs binary_ty) follows the same rules. */
static int arith_dtype(int a, int ra, int b, int rb) {
    int fa = IS_FLOAT(a), fb = IS_FLOAT(b);
    if (fa != fb) return fa ? a : b;
    if (fa) {
        if (a == b) return a;
        if ((ra == 0) != (rb == 0)) return ra ? a : b;
        return MIM_F64;
    }
    int ia = a == MIM_I32 ? 32 : 64, ib = b == MIM_I32 ? 32 : 64;
    int sa = a == MIM_BOOL || a == MIM_U8 || a == MIM_CHAR, sb = b == MIM_BOOL || b == MIM_U8 || b == MIM_CHAR;
    if (ia == 32 && (ib == 32 || sb)) return MIM_I32;
    if (ib == 32 && sa) return MIM_I32;
    if (ia == 32 || ib == 32) {
        if ((ra == 0) != (rb == 0)) return ra ? (ia == 32 ? MIM_I32 : MIM_I64) : (ib == 32 ? MIM_I32 : MIM_I64);
    }
    return MIM_I64;
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
        v->rc = 1; /* another thread won the race: an ordinary value again */
        __atomic_store_n(&live_slot()->n, live_slot()->n + 1, __ATOMIC_RELAXED);
        dealloc(v);
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

/* A variant without fields (None, Leaf, …): one immortal value, shared. */
mim_val *mim_lit_variant(mim_val **slot, const mim_desc *d) {
    mim_val *v = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
    if (v) return v;
    mim_val *n = alloc(MIM_OBJ, 0, 0);
    n->desc = d;
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
    return arith_dtype(a, 0, b, 0);
}

static void retain_children(mim_val *v) {
    if (!IS_HANDLES(v)) return;
    mim_val **h = (mim_val **)v->data;
    uint64_t raw = v->dtype == MIM_OBJ && v->desc ? v->desc->raw : 0;
    for (int64_t i = 0; i < v->len; i++)
        if (!(i < 64 && (raw >> i) & 1)) mim_retain(h[i]);
}

/* ==== shapes (rank ≥ 2) ===================================================
 * Elements of every array are stored flat, in row-major order, so all the
 * elementwise kernels work unchanged. Arrays of rank ≥ 2 keep their
 * dimensions in a side table hung off `desc` (unused by plain arrays). */

#define SHAPE(v) ((int64_t *)(void *)(v)->desc)

/* dimension k of an array (rank ≥ 1) */
static int64_t dim_of(const mim_val *v, int k) { return v->rank >= 2 ? SHAPE(v)[k] : v->len; }

static void set_shape(mim_val *r, int rank, const int64_t *dims) {
    r->rank = rank;
    if (rank >= 2) {
        int64_t *s = (int64_t *)malloc(sizeof(int64_t) * (size_t)rank);
        if (!s) mim_panic("out of memory");
        memcpy(s, dims, sizeof(int64_t) * (size_t)rank);
        r->desc = (const mim_desc *)(void *)s;
    }
}

static mim_val *alloc_shaped(int dt, int rank, const int64_t *dims) {
    int64_t n = 1;
    for (int k = 0; k < rank; k++) n *= dims[k];
    mim_val *r = alloc(dt, rank < 2 ? rank : 1, n);
    if (rank >= 2) set_shape(r, rank, dims);
    return r;
}

/* a fresh array of dtype dt with the shape of src */
static mim_val *alloc_like(int dt, const mim_val *src) {
    if (src->rank < 2) return alloc(dt, src->rank, src->len);
    return alloc_shaped(dt, src->rank, SHAPE(src));
}

static int same_shape(const mim_val *a, const mim_val *b) {
    if (a->rank != b->rank) return 0;
    if (a->rank < 2) return a->len == b->len;
    return memcmp(SHAPE(a), SHAPE(b), sizeof(int64_t) * (size_t)a->rank) == 0;
}

static void fmt_shape(char *buf, size_t cap, const mim_val *v) {
    size_t o = (size_t)snprintf(buf, cap, "[");
    for (int k = 0; k < v->rank && o < cap; k++) o += (size_t)snprintf(buf + o, cap - o, k ? " %lld" : "%lld", (long long)dim_of(v, k));
    if (o < cap) snprintf(buf + o, cap - o, "]");
}

/* rows from..from+n of an array (the leading axis), keeping the other axes */
static mim_val *slice_rows(mim_val *a, int64_t from, int64_t n) {
    int64_t cell = a->len / (dim_of(a, 0) ? dim_of(a, 0) : 1);
    int64_t dims[16];
    for (int k = 0; k < a->rank; k++) dims[k] = dim_of(a, k);
    dims[0] = n;
    mim_val *r = alloc_shaped(a->dtype, a->rank, dims);
    size_t es = elsize(a->dtype);
    memcpy(r->data, (char *)a->data + (size_t)(from * cell) * es, (size_t)(n * cell) * es);
    retain_children(r);
    return r;
}

/* the cell at leading index i: rank r-1 */
static mim_val *cell_at(mim_val *a, int64_t i) {
    int64_t cell = a->len / dim_of(a, 0);
    mim_val *r = alloc_shaped(a->dtype, a->rank - 1, SHAPE(a) + 1);
    size_t es = elsize(a->dtype);
    memcpy(r->data, (char *)a->data + (size_t)(i * cell) * es, (size_t)cell * es);
    retain_children(r);
    return r;
}

mim_val *mim_array_lit(int64_t n, mim_val **elems) {
    if (n == 0) return alloc(MIM_I64, 1, 0);
    int dt = -1, boxed = 0;
    for (int64_t i = 0; i < n; i++) {
        need(elems[i], "array literal");
        if (elems[i]->rank != 0 || IS_HANDLES(elems[i])) boxed = 1;
        else dt = dt == -1 ? elems[i]->dtype : join_dtype(dt, elems[i]->dtype);
    }
    if (boxed) { /* strings, arrays, records: a vector of handles */
        mim_val *v = alloc(MIM_BOXED, 1, n);
        for (int64_t i = 0; i < n; i++) {
            mim_retain(elems[i]);
            ((mim_val **)v->data)[i] = elems[i];
        }
        return v;
    }
    mim_val *v = alloc(dt, 1, n);
    for (int64_t i = 0; i < n; i++) {
        if (IS_FLOAT(dt)) setf(v, i, getf(elems[i], 0));
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
    int f32, i32;   /* a comparison of f32 (i32) numbers: compared as such */
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
        case OP_BAND: ARITH_LOOP(int64_t, int64_t, X & Y); return 1;
        case OP_BOR: ARITH_LOOP(int64_t, int64_t, X | Y); return 1;
        case OP_BXOR: ARITH_LOOP(int64_t, int64_t, X ^ Y); return 1;
        case OP_SHL: ARITH_LOOP(int64_t, int64_t, SHL64(X, Y)); return 1;
        case OP_SHR: ARITH_LOOP(int64_t, int64_t, SHR64(X, Y)); return 1;
        case OP_USHR: ARITH_LOOP(int64_t, int64_t, USHR64(X, Y)); return 1;
        }
    }
    return 0;
}

static void binop_range(void *vc, int64_t lo, int64_t hi) {
    binop_ctx *c = (binop_ctx *)vc;
    if (binop_fast(c, lo, hi)) return;
    int op = c->op;
    int floaty = IS_FLOAT(c->a->dtype) || IS_FLOAT(c->b->dtype);
    for (int64_t i = lo; i < hi; i++) {
        int64_t ia = i * c->sa, ib = i * c->sb;
        mim_val *r = c->r;
        if (floaty || op == OP_DIV) { /* note: OP_IDIV on ints stays integer */
            double x = getf(c->a, ia), y = getf(c->b, ib);
            if (r->dtype == MIM_F32 || (is_cmp(op) && c->f32)) { /* f32 arithmetic: f32 operands */
                x = (double)(float)x;
                y = (double)(float)y;
            }
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
            if (r->dtype == MIM_I32 || (is_cmp(op) && c->i32)) { /* i32 arithmetic: i32 operands */
                x = (int32_t)(uint32_t)x;
                y = (int32_t)(uint32_t)y;
            }
            switch (op) {
            case OP_ADD: seti(r, i, (int64_t)((uint64_t)x + (uint64_t)y)); break;
            case OP_SUB: seti(r, i, (int64_t)((uint64_t)x - (uint64_t)y)); break;
            case OP_MUL: seti(r, i, (int64_t)((uint64_t)x * (uint64_t)y)); break;
            case OP_MOD:
                if (y == 0) mim_panic("integer modulo by zero");
                seti(r, i, floormod(x, y));
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
            case OP_BAND: seti(r, i, x & y); break;
            case OP_BOR: seti(r, i, x | y); break;
            case OP_BXOR: seti(r, i, x ^ y); break;
            case OP_SHL: seti(r, i, SHL64(x, y)); break;
            case OP_SHR: seti(r, i, SHR64(x, y)); break;
            case OP_USHR: seti(r, i, USHR64(x, y)); break;
            }
        }
    }
}

mim_val *mim_std_concat(mim_val *xs, mim_val *ys);
int64_t mim_str_cmp(mim_val *a, mim_val *b);
int64_t mim_equal(const mim_val *a, const mim_val *b);
static mim_val *scalar_at(const mim_val *xs, int64_t i);
static mim_val *slot_get(const mim_val *o, int64_t i);

mim_val *mim_binop(int32_t op, mim_val *a, mim_val *b) {
    need(a, "operator");
    need(b, "operator");
    /* "a" + "b" joins strings (adding character codes is never meant) */
    if (op == OP_ADD && a->dtype == MIM_CHAR && b->dtype == MIM_CHAR && a->rank == 1 && b->rank == 1) return mim_std_concat(a, b);
    if ((op == OP_EQ || op == OP_NE) && a->dtype == MIM_CHAR && b->dtype == MIM_CHAR && a->rank == 1 && b->rank == 1) {
        int eq = a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0;
        return mim_bool(op == OP_EQ ? eq : !eq);
    }
    /* strings order as whole values (byte-wise lexicographic) */
    if (op >= OP_LT && op <= OP_GE && a->dtype == MIM_CHAR && b->dtype == MIM_CHAR && a->rank == 1 && b->rank == 1) {
        int64_t c = mim_str_cmp(a, b);
        return mim_bool(op == OP_LT ? c < 0 : op == OP_LE ? c <= 0 : op == OP_GT ? c > 0 : c >= 0);
    }
    if (IS_HANDLES(a) || IS_HANDLES(b)) {
        /* == / != compare records, variants, tuples and maps as whole values;
         * on two lists (or a list and one value) they compare elementwise */
        if (op != OP_EQ && op != OP_NE) mim_panic("arithmetic is not defined on records or lists of values");
        if (a->dtype == MIM_MAP || b->dtype == MIM_MAP || (a->rank == 0 && b->rank == 0)) {
            int eq = (int)mim_equal(a, b);
            return mim_bool(op == OP_EQ ? eq : !eq);
        }
        if (a->rank == 1 && b->rank == 1 && a->len != b->len) mim_panic("length mismatch in ==");
        int64_t n = a->rank == 1 ? a->len : b->len;
        mim_val *r = alloc(MIM_BOOL, 1, n);
        for (int64_t i = 0; i < n; i++) {
            mim_val *ea = a->rank == 1 ? scalar_at(a, i) : a, *eb = b->rank == 1 ? scalar_at(b, i) : b;
            int eq = (int)mim_equal(ea, eb);
            if (ea != a) mim_release(ea);
            if (eb != b) mim_release(eb);
            seti(r, i, op == OP_EQ ? eq : !eq);
        }
        return r;
    }
    int64_t n;
    const mim_val *like = NULL; /* the operand whose shape the result takes */
    if (a->rank == 0 && b->rank == 0) n = 1;
    else if (a->rank == 0) { n = b->len; like = b; }
    else if (b->rank == 0) { n = a->len; like = a; }
    else {
        if (!same_shape(a, b)) {
            char buf[200];
            if (a->rank == 1 && b->rank == 1) {
                snprintf(buf, sizeof buf, "length mismatch: %lld vs %lld", (long long)a->len, (long long)b->len);
            } else {
                char sa[80], sb[80];
                fmt_shape(sa, sizeof sa, a);
                fmt_shape(sb, sizeof sb, b);
                snprintf(buf, sizeof buf, "shape mismatch: %s vs %s", sa, sb);
            }
            mim_panic(buf);
        }
        n = a->len;
        like = a;
    }
    int floaty = IS_FLOAT(a->dtype) || IS_FLOAT(b->dtype);
    int dt = is_cmp(op) ? MIM_BOOL : arith_dtype(a->dtype, a->rank, b->dtype, b->rank);
    if (op == OP_DIV && !IS_FLOAT(dt)) dt = MIM_F64; /* integers divided: a float */
    if (IS_BITOP(op)) {
        if (floaty || a->dtype == MIM_CHAR || b->dtype == MIM_CHAR) mim_panic("bitwise operators need integers");
        /* bool & bool stays bool (non-short-circuit logic on masks) */
        dt = (a->dtype == MIM_BOOL && b->dtype == MIM_BOOL && op <= OP_BXOR) ? MIM_BOOL : MIM_I64;
    }
    mim_val *r = like ? alloc_like(dt, like) : alloc(dt, 0, n);
    int jt = arith_dtype(a->dtype, a->rank, b->dtype, b->rank);
    binop_ctx c = {op, a, b, r, a->rank == 0 ? 0 : 1, b->rank == 0 ? 0 : 1, jt == MIM_F32, jt == MIM_I32};
    /* the fast loops index x[i] for vectors and x[0] for scalars */
    parallel_for(n, binop_range, &c);
    return r;
}

typedef struct { int op; const mim_val *a; mim_val *r; } unop_ctx;

static void unop_range(void *vc, int64_t lo, int64_t hi) {
    unop_ctx *c = (unop_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) {
        if (c->op == UOP_NOT) seti(c->r, i, getf(c->a, i) == 0);
        else if (c->op == UOP_BNOT) seti(c->r, i, c->a->dtype == MIM_BOOL ? !geti(c->a, i) : ~geti(c->a, i));
        else if (c->op == UOP_ABS) {
            if (IS_FLOAT(c->a->dtype)) setf(c->r, i, fabs(getf(c->a, i)));
            else { int64_t x = geti(c->a, i); seti(c->r, i, x < 0 ? -x : x); }
        }
        else if (IS_FLOAT(c->a->dtype)) setf(c->r, i, -getf(c->a, i));
        else seti(c->r, i, -geti(c->a, i));
    }
}

mim_val *mim_unop(int32_t op, mim_val *a) {
    need(a, "operator");
    if (op == UOP_BNOT && (IS_FLOAT(a->dtype) || a->dtype == MIM_CHAR)) mim_panic("`~` needs an integer");
    int keep = a->dtype == MIM_F64 || a->dtype == MIM_F32 || a->dtype == MIM_I32;
    int dt = op == UOP_NOT || (op == UOP_BNOT && a->dtype == MIM_BOOL) ? MIM_BOOL : (keep ? a->dtype : MIM_I64);
    mim_val *r = alloc_like(dt, a);
    unop_ctx c = {op, a, r};
    parallel_for(a->len, unop_range, &c);
    return r;
}

/* ==== printing =========================================================== */

/* A growable byte buffer: values are formatted into it, then printed or
 * turned into a string (interpolation). */
typedef struct { char *p; size_t n, cap; } sbuf;

static void sb_put(sbuf *b, const char *s, size_t n) {
    if (b->n + n + 1 > b->cap) {
        b->cap = (b->n + n + 1) * 2;
        b->p = (char *)realloc(b->p, b->cap);
        if (!b->p) mim_panic("out of memory");
    }
    memcpy(b->p + b->n, s, n);
    b->n += n;
}
static void sb_str(sbuf *b, const char *s) { sb_put(b, s, strlen(s)); }

/* Decimal digits of an integer (what printf's %lld writes, without printf). */
static size_t fmt_i64(char *out, int64_t x) {
    char tmp[24];
    size_t n = 0;
    uint64_t u = x < 0 ? (uint64_t)0 - (uint64_t)x : (uint64_t)x;
    do {
        tmp[n++] = (char)('0' + u % 10);
        u /= 10;
    } while (u);
    size_t o = 0;
    if (x < 0) out[o++] = '-';
    while (n) out[o++] = tmp[--n];
    return o;
}

static void sb_i64(sbuf *b, int64_t x) {
    char buf[24];
    sb_put(b, buf, fmt_i64(buf, x));
}

static void fmt_elem(sbuf *b, const mim_val *v, int64_t i) {
    char buf[64];
    switch (v->dtype) {
    case MIM_I64:
    case MIM_U8: (void)buf; sb_i64(b, geti(v, i)); break;
    case MIM_BOOL: sb_str(b, geti(v, i) ? "true" : "false"); break;
    case MIM_CHAR: { char c = (char)geti(v, i); sb_put(b, &c, 1); break; }
    case MIM_F64: {
        double x = getf(v, i);
        snprintf(buf, sizeof buf, "%.15g", x);
        sb_str(b, buf);
        if (isfinite(x) && !strpbrk(buf, ".e")) sb_str(b, ".0");
        break;
    }
    case MIM_I32: sb_i64(b, geti(v, i)); break;
    case MIM_F32: {
        /* the shortest text that reads back as the same f32 */
        float x = ((const float *)v->data)[i];
        for (int prec = 6; prec <= 9; prec++) {
            snprintf(buf, sizeof buf, "%.*g", prec, (double)x);
            if (strtof(buf, NULL) == x) break;
        }
        sb_str(b, buf);
        if (isfinite(x) && !strpbrk(buf, ".e")) sb_str(b, ".0");
        break;
    }
    }
}

/* `quoted`: strings nested inside lists/records print with quotes */
static void fmt_val(sbuf *b, const mim_val *v, int quoted) {
    if (!v) { sb_str(b, "()"); return; }
    if (v->dtype == MIM_MAP) {
        int64_t n = ((const int64_t *)(const void *)v->desc)[0]; /* map_meta.count */
        mim_val **h = (mim_val **)v->data;
        if (n == 0) { sb_str(b, "[:]"); return; }
        sb_str(b, "[");
        for (int64_t e = 0; e < n; e++) {
            if (e) sb_str(b, ", ");
            fmt_val(b, h[2 * e], 1);
            sb_str(b, ": ");
            fmt_val(b, h[2 * e + 1], 1);
        }
        sb_str(b, "]");
        return;
    }
    if (v->dtype == MIM_FN) {
        sb_str(b, "<fn ");
        sb_str(b, FN_DESC(v)->name ? FN_DESC(v)->name : "lambda");
        sb_str(b, ">");
        return;
    }
    if (v->dtype == MIM_OBJ) {
        const mim_desc *d = v->desc;
        if (d->variant) {
            sb_str(b, d->variant);
            if (v->len) {
                sb_str(b, "(");
                for (int64_t i = 0; i < v->len; i++) {
                    if (i) sb_str(b, ", ");
                    mim_val *f = slot_get(v, i);
                    fmt_val(b, f, 1);
                    mim_release(f);
                }
                sb_str(b, ")");
            }
        } else {
            sb_str(b, d->type_name);
            sb_str(b, " { ");
            for (int64_t i = 0; i < v->len; i++) {
                if (i) sb_str(b, ", ");
                sb_str(b, d->fields[i]);
                sb_str(b, ": ");
                mim_val *f = slot_get(v, i);
                fmt_val(b, f, 1);
                mim_release(f);
            }
            sb_str(b, " }");
        }
        return;
    }
    if (v->rank == 0) { fmt_elem(b, v, 0); return; }
    if (v->dtype == MIM_CHAR) {
        if (quoted) sb_str(b, "\"");
        sb_put(b, (const char *)v->data, (size_t)v->len);
        if (quoted) sb_str(b, "\"");
        return;
    }
    if (v->rank >= 2) {
        /* one row per line, a bracket per axis, a blank line between blocks:
         * [[1 2 3]
         *  [4 5 6]]                                                        */
        int R = v->rank;
        int64_t row = dim_of(v, R - 1), nrows = row ? v->len / row : 0;
        if (nrows == 0) { sb_str(b, "[]"); return; }
        int64_t S[16], acc = 1; /* S[j]: elements in one block of level j */
        for (int j = R - 1; j >= 0; j--) { acc *= dim_of(v, j); S[j] = acc; }
        for (int64_t r = 0; r < nrows; r++) {
            int64_t flat = r * row;
            int open = 0, close = 0;
            for (int j = 0; j <= R - 2; j++) {
                if (flat % S[j] == 0) open++;
                if ((flat + row) % S[j] == 0) close++;
            }
            if (r) {
                sb_str(b, "\n");
                for (int k = 0; k < open; k++) sb_str(b, "\n");
                for (int k = 0; k < R - 1 - open; k++) sb_str(b, " ");
            }
            for (int k = 0; k <= open; k++) sb_str(b, "[");
            for (int64_t i = 0; i < row; i++) {
                if (i) sb_str(b, " ");
                if (v->dtype == MIM_BOXED) fmt_val(b, ((mim_val **)v->data)[flat + i], 1);
                else fmt_elem(b, v, flat + i);
            }
            for (int k = 0; k <= close; k++) sb_str(b, "]");
        }
        return;
    }
    sb_str(b, "[");
    for (int64_t i = 0; i < v->len; i++) {
        if (i) sb_str(b, " ");
        if (v->dtype == MIM_BOXED) fmt_val(b, ((mim_val **)v->data)[i], 1);
        else fmt_elem(b, v, i);
    }
    sb_str(b, "]");
}

static void print_val(const mim_val *v) {
    sbuf b = {0};
    fmt_val(&b, v, 0);
    if (b.n) fwrite(b.p, 1, b.n, stdout);
    free(b.p);
}

/* ==== strings, records and variants ===================================== */

/* The text of a value, as `print` would show it (strings unquoted). */
mim_val *mim_format(mim_val *v) {
    if (v && v->dtype == MIM_CHAR && v->rank == 1) { mim_retain(v); return v; }
    sbuf b = {0};
    fmt_val(&b, v, 0);
    mim_val *r = mim_str(b.p ? b.p : "", (int64_t)b.n);
    free(b.p);
    return r;
}

/* An integer as text (interpolation of an unboxed i64). */
mim_val *mim_format_i64(int64_t x) {
    char buf[24];
    return mim_str(buf, (int64_t)fmt_i64(buf, x));
}

/* Concatenate `n` strings (interpolation). */
mim_val *mim_concat(int64_t n, mim_val **parts) {
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) total += parts[i]->len;
    mim_val *r = alloc(MIM_CHAR, 1, total);
    int64_t o = 0;
    for (int64_t i = 0; i < n; i++) {
        memcpy((char *)r->data + o, parts[i]->data, (size_t)parts[i]->len);
        o += parts[i]->len;
    }
    return r;
}

/* -1, 0 or 1: byte-wise lexicographic order of two strings */
int64_t mim_str_cmp(mim_val *a, mim_val *b) {
    need(a, "string comparison");
    need(b, "string comparison");
    int64_t n = a->len < b->len ? a->len : b->len;
    int c = memcmp(a->data, b->data, (size_t)n);
    if (c != 0) return c < 0 ? -1 : 1;
    return a->len < b->len ? -1 : a->len > b->len ? 1 : 0;
}

int64_t mim_str_eq(mim_val *a, mim_val *b) {
    need(a, "string comparison");
    need(b, "string comparison");
    return a->dtype == MIM_CHAR && b->dtype == MIM_CHAR && a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0;
}

/* A record or variant from `n` borrowed field values. */
/* ---- raw number fields (mim_desc.raw) ---- */

static inline int slot_raw(const mim_val *o, int64_t i) { return o->desc && i < 64 && ((o->desc->raw >> i) & 1); }

/* field i of a record as a value (owned): a raw number is boxed */
static mim_val *slot_get(const mim_val *o, int64_t i) {
    if (!slot_raw(o, i)) {
        mim_val *f = ((mim_val **)o->data)[i];
        mim_retain(f);
        return f;
    }
    uint64_t bits = ((const uint64_t *)o->data)[i];
    if ((o->desc->fbits >> i) & 1) {
        double x;
        memcpy(&x, &bits, sizeof x);
        return mim_float(x);
    }
    if ((o->desc->bbits >> i) & 1) return mim_bool(bits != 0);
    return mim_int((int64_t)bits);
}

/* store v (borrowed) in the empty field i of a new record */
static void slot_init(mim_val *o, int64_t i, mim_val *v) {
    need(v, "record field");
    if (!slot_raw(o, i)) {
        mim_retain(v);
        ((mim_val **)o->data)[i] = v;
        return;
    }
    if (v->rank != 0 || v->dtype == MIM_CHAR || IS_HANDLES(v)) {
        char buf[160];
        snprintf(buf, sizeof buf, "field `%s` holds a number, found %s", o->desc->fields ? o->desc->fields[i] : "?", dtname(v));
        mim_panic(buf);
    }
    uint64_t bits;
    if ((o->desc->fbits >> i) & 1) {
        double x = IS_FLOAT(v->dtype) ? getf(v, 0) : (double)geti(v, 0);
        memcpy(&bits, &x, sizeof bits);
    } else if ((o->desc->bbits >> i) & 1) {
        bits = IS_FLOAT(v->dtype) ? getf(v, 0) != 0 : geti(v, 0) != 0;
    } else {
        if (IS_FLOAT(v->dtype)) {
            double x = getf(v, 0);
            if (x != (double)(int64_t)x) mim_panic("an integer field cannot hold a fraction");
            bits = (uint64_t)(int64_t)x;
        } else {
            bits = (uint64_t)geti(v, 0);
        }
    }
    ((uint64_t *)o->data)[i] = bits;
}

/* a copy of a record: raw fields copied, handles retained */
static mim_val *obj_clone(const mim_val *o) {
    mim_val *v = alloc(MIM_OBJ, 0, o->len);
    v->desc = o->desc;
    memcpy(v->data, o->data, sizeof(mim_val *) * (size_t)o->len);
    for (int64_t i = 0; i < o->len; i++)
        if (!slot_raw(o, i)) mim_retain(((mim_val **)v->data)[i]);
    return v;
}

mim_val *mim_obj(const mim_desc *d, int64_t n, mim_val **fields) {
    mim_val *v = alloc(MIM_OBJ, 0, n);
    v->desc = d;
    for (int64_t i = 0; i < n; i++) slot_init(v, i, fields[i]);
    return v;
}

/* ==== function values ==================================================== */

mim_val *mim_closure(const mim_fn_desc *d, int64_t nenv, mim_val **env) {
    mim_val *v = alloc(MIM_FN, 0, nenv);
    v->desc = (const mim_desc *)(const void *)d;
    for (int64_t i = 0; i < nenv; i++) {
        mim_retain(env[i]);
        ((mim_val **)v->data)[i] = env[i];
    }
    return v;
}

/* A closure that is pure as long as the function values it captures are:
 * the pure description when they all are, else the other one. (Data never
 * holds functions with effects, E0605, so only direct captures matter.) */
mim_val *mim_closure_checked(const mim_fn_desc *pure, const mim_fn_desc *effects, int64_t nenv, mim_val **env) {
    const mim_fn_desc *d = pure;
    for (int64_t i = 0; i < nenv; i++)
        if (env[i] && env[i]->dtype == MIM_FN && !FN_DESC(env[i])->pure) d = effects;
    return mim_closure(d, nenv, env);
}

mim_val *mim_call(mim_val *f, int32_t argc, mim_val **args) {
    need(f, "function call");
    if (f->dtype != MIM_FN) {
        char buf[128];
        snprintf(buf, sizeof buf, "cannot call a value of type %s", dtname(f));
        mim_panic(buf);
    }
    const mim_fn_desc *d = FN_DESC(f);
    if (d->arity != argc) {
        char buf[160];
        snprintf(buf, sizeof buf, "%s takes %d argument(s), but %d were given", d->name ? d->name : "the function", d->arity, argc);
        mim_panic(buf);
    }
    return d->code((mim_val *const *)f->data, (mim_val *const *)args);
}

static mim_val *obj_check(mim_val *o, const char *what) {
    need(o, what);
    if (o->dtype != MIM_OBJ) {
        char buf[128];
        snprintf(buf, sizeof buf, "%s: expected a record or variant, found %s", what, dtname(o));
        mim_panic(buf);
    }
    return o;
}

int64_t mim_obj_tag(mim_val *o) { return obj_check(o, "match")->desc->tag; }

/* Field `i` (owned). */
mim_val *mim_obj_get(mim_val *o, int64_t i) {
    obj_check(o, "field access");
    if (i < 0 || i >= o->len) mim_panic("field index out of range");
    return slot_get(o, i);
}

/* Field by name, when the record type is only known at run time (owned). */
mim_val *mim_obj_get_named(mim_val *o, const char *name) {
    obj_check(o, "field access");
    const mim_desc *d = o->desc;
    if (d->fields)
        for (int32_t i = 0; i < d->nfields; i++)
            if (strcmp(d->fields[i], name) == 0) return mim_obj_get(o, i);
    /* a tuple's fields are its positions: `t.0`, `t.1` */
    if (d->variant && !d->variant[0] && name[0] >= '0' && name[0] <= '9') {
        char *end;
        long k = strtol(name, &end, 10);
        if (!*end && k < o->len) return mim_obj_get(o, k);
    }
    char buf[160];
    snprintf(buf, sizeof buf, "`%s` has no field `%s`", d->variant ? d->variant : d->type_name, name);
    mim_panic(buf);
    return NULL;
}

/* `let x = v else { … }`: 1 for Some(_)/Ok(_), 0 for None/Err(_). */
int64_t mim_unwrap_ok(mim_val *v) {
    obj_check(v, "`let … else`");
    const char *t = v->desc->type_name;
    if (strcmp(t, "Option") != 0 && strcmp(t, "Result") != 0) {
        char buf[160];
        snprintf(buf, sizeof buf, "`let … else` needs an Option or Result, found %s", t);
        mim_panic(buf);
    }
    return v->desc->tag == 0;
}

/* ---- route patterns: "/users/{id: i64}/posts/{slug}" ---------------------
 * kinds[i]: 0 = literal text (texts[i]), 1 = str capture, 2 = i64, 3 = f64.
 * Captures are non-empty, never contain '/', and are matched shortest-first. */

static int capture_ok(const char *s, int64_t n, int64_t kind) {
    if (kind == 2) {
        int64_t i = (s[0] == '-') ? 1 : 0;
        if (i >= n) return 0;
        for (; i < n; i++)
            if (s[i] < '0' || s[i] > '9') return 0;
        return 1;
    }
    if (kind == 3) {
        char buf[64];
        if (n >= (int64_t)sizeof buf) return 0;
        memcpy(buf, s, (size_t)n);
        buf[n] = 0;
        char *end;
        strtod(buf, &end);
        return end == buf + n;
    }
    return 1;
}

static int route_rec(const char *s, int64_t len, int64_t si, int64_t pi, int64_t n, const int64_t *kinds,
                     const char *const *texts, int64_t *cs, int64_t *ce, int ci) {
    if (pi == n) return si == len;
    if (kinds[pi] == 0) {
        int64_t tl = (int64_t)strlen(texts[pi]);
        if (si + tl > len || memcmp(s + si, texts[pi], (size_t)tl) != 0) return 0;
        return route_rec(s, len, si + tl, pi + 1, n, kinds, texts, cs, ce, ci);
    }
    for (int64_t e = si + 1; e <= len; e++) {
        if (s[e - 1] == '/') break;
        if (pi + 1 == n && e != len) continue; /* the last part takes the rest */
        if (!capture_ok(s + si, e - si, kinds[pi])) continue;
        cs[ci] = si;
        ce[ci] = e;
        if (route_rec(s, len, e, pi + 1, n, kinds, texts, cs, ce, ci + 1)) return 1;
    }
    return 0;
}

/* Returns 1 on a match; when `out` is non-NULL it receives the captures (owned). */
int64_t mim_route(mim_val *str, int64_t n, const int64_t *kinds, const char *const *texts, mim_val **out) {
    need(str, "route pattern");
    if (str->dtype != MIM_CHAR || str->rank != 1) return 0;
    int64_t cs[32], ce[32];
    if (n > 32) mim_panic("route pattern has too many parts");
    if (!route_rec((const char *)str->data, str->len, 0, 0, n, kinds, texts, cs, ce, 0)) return 0;
    if (out) {
        const char *s = (const char *)str->data;
        int k = 0;
        for (int64_t i = 0; i < n; i++) {
            if (kinds[i] == 0) continue;
            int64_t a = cs[k], len = ce[k] - cs[k];
            char buf[64];
            if (kinds[i] == 1) out[k] = mim_str(s + a, len);
            else {
                memcpy(buf, s + a, (size_t)(len < 63 ? len : 63));
                buf[len < 63 ? len : 63] = 0;
                out[k] = kinds[i] == 2 ? mim_int(strtoll(buf, NULL, 10)) : mim_float(strtod(buf, NULL));
            }
            k++;
        }
    }
    return 1;
}

/* `{ o with field_i: v }`. CONSUMES `o` (borrows `v`): when the caller held
 * the only reference the record is updated in place, otherwise it is copied. */
/* A record to update in place (owned): itself when nothing else holds it,
 * else a copy (the original released). For compiled `{ r with … }`. */
mim_val *mim_obj_unique(mim_val *o) {
    obj_check(o, "record update");
    if (__atomic_load_n(&o->rc, __ATOMIC_ACQUIRE) == 1) return o;
    mim_val *r = obj_clone(o);
    mim_release(o);
    return r;
}

mim_val *mim_obj_with(mim_val *o, int64_t i, mim_val *v) {
    obj_check(o, "record update");
    need(v, "record update");
    if (i < 0 || i >= o->len) mim_panic("field index out of range");
    mim_val *r;
    if (__atomic_load_n(&o->rc, __ATOMIC_ACQUIRE) == 1) {
        r = o;
    } else {
        r = obj_clone(o);
        mim_release(o);
    }
    mim_val **h = (mim_val **)r->data;
    if (slot_raw(r, i)) {
        slot_init(r, i, v); /* overwrites the number */
        return r;
    }
    mim_retain(v);
    mim_release(h[i]);
    h[i] = v;
    return r;
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
    if (c->pf && c->a->dtype != MIM_F64) {
        /* f32: every step rounded to f32 (the result of f32 arithmetic) */
        int r32 = c->a->dtype == MIM_F32;
        double acc = getf(c->a, lo);
        for (int64_t i = lo + 1; i < hi; i++) {
            double x = getf(c->a, i);
            switch (c->prim) {
            case P_SUM: acc += x; break;
            case P_PRODUCT: acc *= x; break;
            case P_MAX: acc = x > acc ? x : acc; break;
            case P_MIN: acc = x < acc ? x : acc; break;
            }
            if (r32) acc = (double)(float)acc;
        }
        c->pf[slot] = acc;
    } else if (c->pf) {
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

typedef struct { int prim; const mim_val *a; mim_val *r; int64_t row; } rows_ctx;

/* reduce each row (the last axis) of a rank ≥ 2 array */
static void reduce_rows_range(void *vc, int64_t lo, int64_t hi) {
    rows_ctx *c = (rows_ctx *)vc;
    int64_t row = c->row;
    for (int64_t i = lo; i < hi; i++) {
        int64_t base = i * row;
        if (IS_FLOAT(c->r->dtype)) {
            int r32 = c->r->dtype == MIM_F32;
            double acc = c->prim == P_PRODUCT ? 1.0 : c->prim == P_SUM ? 0.0 : getf(c->a, base);
            for (int64_t j = 0; j < row; j++) {
                double x = getf(c->a, base + j);
                switch (c->prim) {
                case P_SUM: acc += x; break;
                case P_PRODUCT: acc *= x; break;
                case P_MAX: acc = x > acc ? x : acc; break;
                case P_MIN: acc = x < acc ? x : acc; break;
                }
                if (r32) acc = (double)(float)acc;
            }
            setf(c->r, i, acc);
        } else {
            int64_t acc = c->prim == P_PRODUCT ? 1 : c->prim == P_SUM ? 0 : geti(c->a, base);
            for (int64_t j = 0; j < row; j++) {
                int64_t x = geti(c->a, base + j);
                switch (c->prim) {
                case P_SUM: acc = (int64_t)((uint64_t)acc + (uint64_t)x); break;
                case P_PRODUCT: acc = (int64_t)((uint64_t)acc * (uint64_t)x); break;
                case P_MAX: acc = x > acc ? x : acc; break;
                case P_MIN: acc = x < acc ? x : acc; break;
                }
            }
            seti(c->r, i, acc);
        }
    }
}

static mim_val *reduce(int prim, mim_val *a) {
    if (IS_HANDLES(a)) mim_panic("cannot reduce a list of records/strings/lists with an arithmetic primitive");
    if (a->rank == 0) { mim_retain(a); return a; }
    if (a->rank >= 2) {
        /* +/ ×/ ⌈/ ⌊/ on a matrix reduce each row: the result drops the last axis */
        int64_t row = dim_of(a, a->rank - 1);
        if (row == 0 && (prim == P_MAX || prim == P_MIN)) mim_panic("maximum/minimum of empty rows");
        int rdt = IS_FLOAT(a->dtype) || a->dtype == MIM_I32 ? a->dtype : MIM_I64;
        mim_val *r = alloc_shaped(rdt, a->rank - 1, SHAPE(a));
        rows_ctx c = {prim, a, r, row};
        parallel_for(r->len, reduce_rows_range, &c);
        return r;
    }
    if (a->len == 0) {
        if (prim == P_SUM) return mim_int(0);
        if (prim == P_PRODUCT) return mim_int(1);
        mim_panic("maximum/minimum of an empty array");
    }
    int f = IS_FLOAT(a->dtype), r32 = a->dtype == MIM_F32;
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
            if (r32) acc = (double)(float)acc;
        }
        r = r32 ? mim_float32(acc) : mim_float(acc);
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
        r = a->dtype == MIM_I32 ? mim_int32(acc) : mim_int(acc); /* (i32: wrapping, as each step would) */
        free(c.pi);
    }
    return r;
}

/* ==== structural primitives ============================================== */

static mim_val *slice(mim_val *a, int64_t from, int64_t n) {
    mim_val *r = alloc(a->dtype, 1, n);
    memcpy(r->data, (char *)a->data + from * elsize(a->dtype), (size_t)n * elsize(a->dtype));
    retain_children(r);
    return r;
}

static void iota_range(void *vc, int64_t lo, int64_t hi) {
    int64_t *d = (int64_t *)vc;
    for (int64_t i = lo; i < hi; i++) d[i] = i;
}

/* ⍉: reverse the order of the axes (rows ↔ columns for a matrix) */
typedef struct { const mim_val *a; mim_val *r; } tr_ctx;

static void transpose2_range(void *vc, int64_t lo, int64_t hi) {
    tr_ctx *c = (tr_ctx *)vc;
    int64_t rows = dim_of(c->a, 0), cols = dim_of(c->a, 1);
    size_t es = elsize(c->a->dtype);
    for (int64_t j = lo; j < hi; j++) /* result row j = source column j */
        for (int64_t i = 0; i < rows; i++)
            memcpy((char *)c->r->data + (size_t)(j * rows + i) * es, (char *)c->a->data + (size_t)(i * cols + j) * es, es);
}

static mim_val *transpose(mim_val *a) {
    if (a->rank < 2) { mim_retain(a); return a; }
    int R = a->rank;
    int64_t dims[16];
    for (int k = 0; k < R; k++) dims[k] = dim_of(a, R - 1 - k);
    mim_val *r = alloc_shaped(a->dtype, R, dims);
    if (R == 2) {
        tr_ctx c = {a, r};
        parallel_for(dim_of(a, 1), transpose2_range, &c);
    } else {
        /* general rank: walk the result, map each index back */
        size_t es = elsize(a->dtype);
        int64_t stride[16], idx[16] = {0};
        stride[R - 1] = 1;
        for (int k = R - 2; k >= 0; k--) stride[k] = stride[k + 1] * dim_of(a, k + 1);
        for (int64_t f = 0; f < r->len; f++) {
            int64_t src = 0;
            for (int k = 0; k < R; k++) src += idx[k] * stride[R - 1 - k];
            memcpy((char *)r->data + (size_t)f * es, (char *)a->data + (size_t)src * es, es);
            for (int k = R - 1; k >= 0; k--) {
                if (++idx[k] < dims[k]) break;
                idx[k] = 0;
            }
        }
    }
    retain_children(r);
    return r;
}

/* data ⥊ dims: the elements of `data` (flat, repeated as needed) in a new shape */
static mim_val *reshape(mim_val *a, mim_val *b) {
    if (a->dtype == MIM_OBJ || a->dtype == MIM_MAP || a->dtype == MIM_FN)
        mim_panic("⥊ (reshape) works on arrays of numbers, bools, characters and strings");
    if (IS_FLOAT(b->dtype) || IS_HANDLES(b) || b->rank > 1) mim_panic("⥊ (reshape): the new shape must be a list of integers, e.g. [2 3]");
    int R = b->rank == 0 ? 1 : (int)b->len;
    if (R == 0) mim_panic("⥊ (reshape): the shape needs at least one dimension");
    if (R > 16) mim_panic("⥊ (reshape): at most 16 dimensions");
    int64_t dims[16];
    for (int k = 0; k < R; k++) {
        dims[k] = geti(b, b->rank == 0 ? 0 : k);
        if (dims[k] < 0) mim_panic("⥊ (reshape): negative dimension");
    }
    mim_val *r = alloc_shaped(a->dtype, R, dims);
    if (r->len == 0) return r;
    int64_t n = a->rank == 0 ? 1 : a->len;
    if (n == 0) mim_panic("⥊ (reshape): cannot fill a shape from an empty array");
    size_t es = elsize(a->dtype);
    if (n >= r->len) {
        memcpy(r->data, a->data, (size_t)r->len * es);
    } else {
        for (int64_t f = 0; f < r->len; f += n) {
            int64_t m = r->len - f < n ? r->len - f : n;
            memcpy((char *)r->data + (size_t)f * es, a->data, (size_t)m * es);
        }
    }
    retain_children(r);
    return r;
}

/* a ⊠ b: matrix product. Vectors act as a row (left) or a column (right);
 * two vectors give their dot product. Rows of the result run in parallel. */
typedef struct { const double *x, *y; double *z; int64_t n, k, m; } mm_f_ctx;
typedef struct { const int64_t *x, *y; int64_t *z; int64_t n, k, m; } mm_i_ctx;

/* The fallback kernel: each task takes a range of 8-column panels of y,
 * packs the panel contiguously (one cache line per step instead of one
 * page) and computes 4×8 tiles of z in registers. Copies of the context in
 * locals + `restrict` let the compiler keep everything in vector registers. */
#define MM_MR 4
#define MM_NR 8
#define MM_KERNEL(NAME, CTX, T, MADD)                                                              \
    static void NAME(void *vc, int64_t lo, int64_t hi) {                                           \
        const CTX *c = (const CTX *)vc;                                                            \
        const T *restrict x = c->x, *restrict y = c->y;                                            \
        T *restrict z = c->z;                                                                      \
        const int64_t n = c->n, k = c->k, m = c->m;                                                \
        T *pk = (T *)malloc(sizeof(T) * (size_t)(k ? k : 1) * MM_NR);                              \
        for (int64_t q = lo; q < hi; q++) {                                                        \
            const int64_t j0 = q * MM_NR, w = m - j0 < MM_NR ? m - j0 : MM_NR;                     \
            if (w == MM_NR) {                                                                      \
                for (int64_t p = 0; p < k; p++)                                                    \
                    for (int cc = 0; cc < MM_NR; cc++) pk[p * MM_NR + cc] = y[p * m + j0 + cc];    \
                int64_t i = 0;                                                                     \
                for (; i + MM_MR <= n; i += MM_MR) {                                               \
                    T acc[MM_MR][MM_NR] = {{0}};                                                   \
                    const T *x0 = x + i * k;                                                       \
                    for (int64_t p = 0; p < k; p++) {                                              \
                        const T *yr = pk + p * MM_NR;                                              \
                        for (int r = 0; r < MM_MR; r++) {                                          \
                            const T xv = x0[r * k + p];                                            \
                            for (int cc = 0; cc < MM_NR; cc++) acc[r][cc] = MADD(acc[r][cc], xv, yr[cc]); \
                        }                                                                          \
                    }                                                                              \
                    for (int r = 0; r < MM_MR; r++)                                                \
                        for (int cc = 0; cc < MM_NR; cc++) z[(i + r) * m + j0 + cc] = acc[r][cc];  \
                }                                                                                  \
                for (; i < n; i++) {                                                               \
                    T acc[MM_NR] = {0};                                                            \
                    for (int64_t p = 0; p < k; p++)                                                \
                        for (int cc = 0; cc < MM_NR; cc++) acc[cc] = MADD(acc[cc], x[i * k + p], pk[p * MM_NR + cc]); \
                    for (int cc = 0; cc < MM_NR; cc++) z[i * m + j0 + cc] = acc[cc];               \
                }                                                                                  \
            } else { /* the last, narrower panel */                                                \
                for (int64_t i = 0; i < n; i++)                                                    \
                    for (int64_t j = j0; j < m; j++) {                                             \
                        T a = 0;                                                                   \
                        for (int64_t p = 0; p < k; p++) a = MADD(a, x[i * k + p], y[p * m + j]);   \
                        z[i * m + j] = a;                                                          \
                    }                                                                              \
            }                                                                                      \
        }                                                                                          \
        free(pk);                                                                                  \
    }

#define MADD_F(a, x, y) ((a) + (x) * (y))
#define MADD_I(a, x, y) ((int64_t)((uint64_t)(a) + (uint64_t)(x) * (uint64_t)(y)))
MM_KERNEL(mm_f_range, mm_f_ctx, double, MADD_F)
MM_KERNEL(mm_i_range, mm_i_ctx, int64_t, MADD_I)

#ifdef MIM_DLBLAS
typedef void (*dgemm_fn)(int, int, int, int, int, int, double, const double *, int, const double *, int, double, double *, int);
static dgemm_fn blas_dgemm(void) {
    static dgemm_fn fn;
    static int tried;
    if (__atomic_load_n(&tried, __ATOMIC_ACQUIRE)) return fn;
    if (!getenv("MIMIC_NO_BLAS")) {
        const char *libs[] = {
#ifdef __APPLE__
            "/System/Library/Frameworks/Accelerate.framework/Accelerate",
#else
            "libopenblas.so.0", "libopenblas.so", "libblis.so.4",
#endif
            NULL};
        for (int i = 0; libs[i] && !fn; i++) {
            void *h = dlopen(libs[i], RTLD_LAZY | RTLD_LOCAL);
            if (h) fn = (dgemm_fn)dlsym(h, "cblas_dgemm");
        }
    }
    __atomic_store_n(&tried, 1, __ATOMIC_RELEASE);
    return fn;
}
#endif

/* the elements of v as a dense f64 or i64 buffer (borrowed when already so) */
static void *dense(const mim_val *v, int as_f64, int *owned) {
    int native = as_f64 ? v->dtype == MIM_F64 : v->dtype == MIM_I64;
    *owned = !native;
    if (native) return (void *)v->data;
    void *buf = malloc((size_t)(v->len ? v->len : 1) * 8);
    for (int64_t i = 0; i < v->len; i++) {
        if (as_f64) ((double *)buf)[i] = getf(v, i);
        else ((int64_t *)buf)[i] = geti(v, i);
    }
    return buf;
}

static mim_val *matmul(mim_val *a, mim_val *b) {
    if (IS_HANDLES(a) || IS_HANDLES(b) || a->dtype == MIM_CHAR || b->dtype == MIM_CHAR)
        mim_panic("⊠ (matmul) works on arrays of numbers");
    if (a->rank == 0 || b->rank == 0 || a->rank > 2 || b->rank > 2) mim_panic("⊠ (matmul) multiplies vectors and matrices");
    int64_t n = a->rank == 2 ? dim_of(a, 0) : 1, k = a->rank == 2 ? dim_of(a, 1) : a->len;
    int64_t k2 = b->rank == 2 ? dim_of(b, 0) : b->len, m = b->rank == 2 ? dim_of(b, 1) : 1;
    if (k != k2) {
        char buf[200], sa[80], sb[80];
        fmt_shape(sa, sizeof sa, a);
        fmt_shape(sb, sizeof sb, b);
        snprintf(buf, sizeof buf, "⊠ (matmul): shapes %s and %s do not fit (%lld columns vs %lld rows)", sa, sb, (long long)k, (long long)k2);
        mim_panic(buf);
    }
    int f = a->dtype == MIM_F64 || b->dtype == MIM_F64;
    int oa, ob;
    void *x = dense(a, f, &oa), *y = dense(b, f, &ob);
    /* result shape: [n m], dropping the axes that came from vectors */
    int64_t dims[2];
    int R = 0;
    if (a->rank == 2) dims[R++] = n;
    if (b->rank == 2) dims[R++] = m;
    mim_val *r = R == 0 ? alloc(f ? MIM_F64 : MIM_I64, 0, 1) : alloc_shaped(f ? MIM_F64 : MIM_I64, R, dims);
    int done = 0;
#ifdef MIM_DLBLAS
    /* large float products: the platform BLAS (on Apple: the matrix units) */
    if (f && n * k * m >= 32768 && n < INT32_MAX && k < INT32_MAX && m < INT32_MAX) {
        dgemm_fn g = blas_dgemm();
        if (g) {
            /* 101 = row-major, 111 = no transpose */
            g(101, 111, 111, (int)n, (int)m, (int)k, 1.0, (const double *)x, (int)k, (const double *)y, (int)m, 0.0, (double *)r->data, (int)m);
            done = 1;
        }
    }
#endif
    if (!done) {
        /* tasks are 8-column panels; about 256k multiply-adds per task */
        int64_t panels = (m + MM_NR - 1) / MM_NR, per = n * k * MM_NR;
        int64_t grain = per > 0 ? (262144 + per - 1) / per : panels;
        if (grain < 1) grain = 1;
        if (f) {
            mm_f_ctx c = {(const double *)x, (const double *)y, (double *)r->data, n, k, m};
            parallel_for_grain(panels, grain, mm_f_range, &c);
        } else {
            mm_i_ctx c = {(const int64_t *)x, (const int64_t *)y, (int64_t *)r->data, n, k, m};
            parallel_for_grain(panels, grain, mm_i_range, &c);
        }
    }
    if (oa) free(x);
    if (ob) free(y);
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
        if (a->rank != 0 || IS_FLOAT(a->dtype)) mim_panic("⍳ (iota) expects an integer scalar");
        int64_t n = geti(a, 0);
        if (n < 0) mim_panic("⍳ (iota) of a negative number");
        mim_val *r = alloc(MIM_I64, 1, n);
        parallel_for(n, iota_range, r->data);
        return r;
    }
    case P_SHAPE: {
        if (a->rank == 0) return alloc(MIM_I64, 1, 0);
        if (a->rank >= 2) {
            mim_val *r = alloc(MIM_I64, 1, a->rank);
            memcpy(r->data, SHAPE(a), sizeof(int64_t) * (size_t)a->rank);
            return r;
        }
        mim_val *r = alloc(MIM_I64, 1, 1);
        ((int64_t *)r->data)[0] = a->len;
        return r;
    }
    case P_REVERSE: {
        if (a->rank == 0) { mim_retain(a); return a; }
        if (a->rank >= 2) { /* reverse the rows (leading axis) */
            int64_t n = dim_of(a, 0), cell = a->len / (n ? n : 1);
            mim_val *r = alloc_like(a->dtype, a);
            size_t cb = (size_t)cell * elsize(a->dtype);
            for (int64_t i = 0; i < n; i++) memcpy((char *)r->data + (size_t)i * cb, (char *)a->data + (size_t)(n - 1 - i) * cb, cb);
            retain_children(r);
            return r;
        }
        mim_val *r = alloc(a->dtype, 1, a->len);
        size_t es = elsize(a->dtype);
        for (int64_t i = 0; i < a->len; i++)
            memcpy((char *)r->data + i * es, (char *)a->data + (a->len - 1 - i) * es, es);
        retain_children(r);
        return r;
    }
    case P_SUM:
    case P_PRODUCT:
    case P_MAX:
    case P_MIN: return reduce(prim, a);
    case P_LEN:
        if (a->dtype == MIM_MAP) return mim_int(((const int64_t *)(const void *)a->desc)[0]);
        return mim_int(a->rank == 0 ? 1 : dim_of(a, 0)); /* the leading axis */
    case P_TRANSPOSE: return transpose(a);
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
        if (b->rank != 0 || IS_FLOAT(b->dtype)) mim_panic("take/drop count must be an integer scalar");
        if (a->rank == 0) mim_panic("take/drop expects a vector on the left");
        int64_t n = geti(b, 0), len = dim_of(a, 0);
        int64_t k = n < 0 ? -n : n;
        if (k > len) k = len;
        if (a->rank >= 2) { /* rows */
            if (prim == P_TAKE) return n >= 0 ? slice_rows(a, 0, k) : slice_rows(a, len - k, k);
            return n >= 0 ? slice_rows(a, k, len - k) : slice_rows(a, 0, len - k);
        }
        if (prim == P_TAKE) return n >= 0 ? slice(a, 0, k) : slice(a, len - k, k);
        return n >= 0 ? slice(a, k, len - k) : slice(a, 0, len - k);
    }
    case P_RESHAPE: return reshape(a, b);
    case P_MATMUL: return matmul(a, b);
    case P_RANGE: {
        if (a->rank != 0 || b->rank != 0 || IS_FLOAT(a->dtype) || IS_FLOAT(b->dtype))
            mim_panic("‥ (range) expects two integer scalars");
        int64_t lo = geti(a, 0), hi = geti(b, 0), n = hi > lo ? hi - lo : 0;
        mim_val *r = alloc(MIM_I64, 1, n);
        for (int64_t i = 0; i < n; i++) ((int64_t *)r->data)[i] = lo + i;
        return r;
    }
    case P_KEEP: {
        if (a->rank == 0 || b->rank == 0) mim_panic("⌿ (keep) expects two vectors");
        if (a->rank >= 2 || b->rank >= 2) mim_panic("⌿ (keep) works on vectors; select rows with indexing or ↑ ↓");
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
        retain_children(r);
        return r;
    }
    }
    mim_panic("unknown dyadic primitive");
    return NULL;
}

/* ==== each =============================================================== */

static mim_val *scalar_at(const mim_val *xs, int64_t i) {
    switch (xs->dtype) {
    case MIM_OBJ: return slot_get(xs, i);
    case MIM_BOXED: {
        mim_val *e = ((mim_val **)xs->data)[i];
        mim_retain(e);
        return e;
    }
    case MIM_F64: return mim_float(getf(xs, i));
    case MIM_F32: return mim_float32(getf(xs, i));
    case MIM_I32: return mim_int32(geti(xs, i));
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
    if (c->xs->dtype == MIM_BOXED) { /* handles: lent to f as they are */
        mim_val **h = (mim_val **)c->xs->data;
        for (int64_t i = lo; i < hi; i++) c->out[i] = c->f(h[i]);
        return;
    }
    for (int64_t i = lo; i < hi; i++) {
        mim_val *e = scalar_at(c->xs, i);
        mim_val *r = c->f(e);
        mim_release(e);
        c->out[i] = r;
    }
}

static mim_val *list_take(int64_t n, mim_val **xs);

/* `pure` functions (no print / FFI, transitively — decided by the compiler)
 * run in parallel; impure ones run in order on the calling thread. */
/* `pure` of the ¨ entry points: 0 in order; 1 a pure function (spread over
 * the cores); 2 effects that may overlap, such as waiting on the network:
 * each element is a task of its own on the event loop, so thousands can wait
 * at once (on the cores when there is no event loop). In mimic_io.c. */
static int run_on_loop(int64_t n, void (*range)(void *, int64_t, int64_t), void *ctx);

mim_val *mim_each(mim_val *xs, mim_fn1 f, int32_t pure) {
    need(xs, "¨ (each)");
    if (xs->rank == 0) return f(xs);
    int64_t n = xs->len;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    each_ctx c = {xs, f, tmp};
    if (pure == 2 && n > 1 && run_on_loop(n, each_range, &c)) {
    } else if (pure) parallel_for_grain(n, fine_grain(n), each_range, &c);
    else each_range(&c, 0, n);
    /* a function run for its effects only: `xs ¨ (x => chan::close(x))` is () */
    int64_t units = 0;
    for (int64_t i = 0; i < n; i++) units += tmp[i] == NULL;
    if (units) {
        for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
        free(tmp);
        if (units != n) mim_panic("¨ (each): the function returned a value for some elements and nothing for others");
        return NULL;
    }
    mim_val *r = list_take(n, tmp); /* the results move in */
    free(tmp);
    if (xs->rank >= 2) set_shape(r, xs->rank, SHAPE(xs)); /* ¨ keeps the shape */
    return r;
}

/* `xs ¨ f` where f builds part of a screen (a pure function of the
 * element: the compiler decides): the results of the last run at this call
 * site are kept, and an element that is the very same value as then gets
 * its old result again instead of a new call. Values never change, and the
 * kept elements stay alive (so their addresses cannot be reused by other
 * values): the same address means the same value. Rows of a list whose
 * data did not change are not built again, and ui::diff skips them. */
typedef struct memo_tab {
    int64_t cap; /* a power of two; 0 = empty */
    mim_val **key, **val;
    mim_val *fn; /* the closure that made them (its captures), or NULL */
} memo_tab;

static inline uint64_t memo_hash(const void *p) {
    uint64_t x = (uint64_t)(uintptr_t)p >> 4;
    x *= 0x9E3779B97F4A7C15ULL;
    return x ^ (x >> 29);
}

static mim_val *memo_find(const memo_tab *t, const mim_val *k) {
    if (!t || !t->cap) return NULL;
    uint64_t m = (uint64_t)t->cap - 1;
    for (uint64_t i = memo_hash(k) & m;; i = (i + 1) & m) {
        if (t->key[i] == k) return t->val[i];
        if (!t->key[i]) return NULL;
    }
}

static void memo_free(memo_tab *t) {
    if (!t) return;
    for (int64_t i = 0; i < t->cap; i++)
        if (t->key[i]) {
            mim_release(t->key[i]);
            mim_release(t->val[i]);
        }
    free(t->key);
    free(t->val);
    mim_release(t->fn);
    free(t);
}

/* every call site's slot, to release what they keep at exit */
static void **g_memo_sites[1024];
static int32_t g_memo_nsites = 0, g_memo_lock = 0;

static void memo_register(void **site) {
    while (__atomic_exchange_n(&g_memo_lock, 1, __ATOMIC_ACQUIRE)) {
    }
    int known = 0;
    for (int i = 0; i < g_memo_nsites; i++) known |= g_memo_sites[i] == site;
    if (!known && g_memo_nsites < 1024) g_memo_sites[g_memo_nsites++] = site;
    __atomic_store_n(&g_memo_lock, 0, __ATOMIC_RELEASE);
}

static void memo_release_all(void) {
    for (int i = 0; i < g_memo_nsites; i++) memo_free((memo_tab *)__atomic_exchange_n(g_memo_sites[i], NULL, __ATOMIC_ACQ_REL));
}

typedef struct {
    mim_val **in;
    mim_fn1 f;
    mim_val *closure; /* instead of f */
    mim_val **out;
    const memo_tab *old;
} memo_ctx;

static void memo_range(void *vc, int64_t lo, int64_t hi) {
    memo_ctx *c = (memo_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) {
        mim_val *r = memo_find(c->old, c->in[i]);
        if (r) {
            mim_retain(r);
            c->out[i] = r;
        } else {
            c->out[i] = c->closure ? mim_call(c->closure, 1, &c->in[i]) : c->f(c->in[i]);
        }
    }
}

/* The same value for a capture: the same object, an equal number, or the
 * same text. */
static int memo_same(const mim_val *a, const mim_val *b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    if (a->rank == 0 && b->rank == 0 && !IS_HANDLES(a) && !IS_HANDLES(b) && a->dtype != MIM_CHAR && b->dtype != MIM_CHAR) return (int)mim_equal(a, b);
    if (a->dtype == MIM_CHAR && b->dtype == MIM_CHAR && a->rank == 1 && b->rank == 1)
        return a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0;
    return 0;
}

/* Does closure f capture the same values as the one that made `old`? */
static int memo_same_closure(const memo_tab *old, const mim_val *f) {
    const mim_val *g = old->fn;
    if (!f || !g) return f == g;
    if (FN_DESC(f)->code != FN_DESC(g)->code || f->len != g->len) return 0;
    for (int64_t i = 0; i < f->len; i++)
        if (!memo_same(((mim_val *const *)f->data)[i], ((mim_val *const *)g->data)[i])) return 0;
    return 1;
}

static mim_val *each_memo_run(mim_val *xs, mim_fn1 f, mim_val *closure, int32_t pure, void **site);
mim_val *mim_each_fn(mim_val *xs, mim_val *f);

mim_val *mim_each_memo(mim_val *xs, mim_fn1 f, int32_t pure, void **site) {
    need(xs, "¨ (each)");
    if (xs->dtype != MIM_BOXED || xs->rank != 1 || xs->len == 0) return mim_each(xs, f, pure);
    return each_memo_run(xs, f, NULL, pure, site);
}

/* The same with a closure (a lambda that captures): the kept rows count
 * only while it captures the same values. */
mim_val *mim_each_fn_memo(mim_val *xs, mim_val *f, void **site) {
    need(xs, "¨ (each)");
    need(f, "¨ (each)");
    if (xs->dtype != MIM_BOXED || xs->rank != 1 || xs->len == 0 || f->dtype != MIM_FN || FN_DESC(f)->pure != 1) return mim_each_fn(xs, f);
    return each_memo_run(xs, NULL, f, 1, site);
}

static mim_val *each_memo_run(mim_val *xs, mim_fn1 f, mim_val *closure, int32_t pure, void **site) {
    int64_t n = xs->len;
    memo_register(site);
    memo_tab *old = (memo_tab *)__atomic_exchange_n(site, NULL, __ATOMIC_ACQ_REL);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    /* rows made with other captures do not count */
    memo_ctx c = {(mim_val **)xs->data, f, closure, tmp, old && memo_same_closure(old, closure) ? old : NULL};
    if (pure) parallel_for_grain(n, fine_grain(n), memo_range, &c);
    else memo_range(&c, 0, n);
    for (int64_t i = 0; i < n; i++)
        if (!tmp[i]) { /* a function run for its effects: nothing to keep */
            memo_free(old);
            for (int64_t k = 0; k < n; k++) mim_release(tmp[k]);
            free(tmp);
            mim_panic("¨ (each): a function that builds a screen returned nothing");
        }
    /* this run's elements and results, for the next run */
    memo_tab *t = (memo_tab *)calloc(1, sizeof(memo_tab));
    int64_t cap = 16;
    while (cap < 2 * n) cap *= 2;
    t->cap = cap;
    t->fn = closure;
    mim_retain(closure);
    t->key = (mim_val **)calloc((size_t)cap, sizeof(mim_val *));
    t->val = (mim_val **)calloc((size_t)cap, sizeof(mim_val *));
    for (int64_t i = 0; i < n; i++) {
        mim_val *k = c.in[i];
        uint64_t m = (uint64_t)cap - 1, j = memo_hash(k) & m;
        while (t->key[j] && t->key[j] != k) j = (j + 1) & m;
        if (t->key[j]) continue; /* the same element twice: once is enough */
        mim_retain(k);
        mim_retain(tmp[i]);
        t->key[j] = k;
        t->val[j] = tmp[i];
    }
    memo_free(old);
    memo_free((memo_tab *)__atomic_exchange_n(site, t, __ATOMIC_ACQ_REL)); /* (another thread's, if any) */
    mim_val *r = list_take(n, tmp);
    free(tmp);
    return r;
}

/* Are a and b the very same value (not merely equal)? ui::diff skips such
 * subtrees: a row reused by mim_each_memo is unchanged. */
mim_val *mim_std_ui_identical(mim_val *a, mim_val *b) { return mim_bool(a == b); }

/* `xs ¨ f` over a vector of numbers (i64 or f64, as the compiler knows it
 * is): f takes each element's 8 bytes as they are, no box. */
typedef mim_val *(*mim_fn_raw)(int64_t);
typedef struct {
    const int64_t *data;
    mim_fn_raw f;
    mim_val **out;
} each_raw_ctx;

static void each_raw_range(void *vc, int64_t lo, int64_t hi) {
    each_raw_ctx *c = (each_raw_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) c->out[i] = c->f(c->data[i]);
}

mim_val *mim_each_raw(mim_val *xs, mim_fn_raw f, int32_t dt, int32_t pure) {
    need(xs, "¨ (each)");
    if (xs->dtype != dt || xs->rank != 1) mim_panic("internal error: ¨ over numbers of another type");
    int64_t n = xs->len;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    each_raw_ctx c = {(const int64_t *)xs->data, f, tmp};
    if (pure == 2 && n > 1 && run_on_loop(n, each_raw_range, &c)) {
    } else if (pure) parallel_for_grain(n, fine_grain(n), each_raw_range, &c);
    else each_raw_range(&c, 0, n);
    int64_t units = 0;
    for (int64_t i = 0; i < n; i++) units += tmp[i] == NULL;
    if (units) {
        for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
        free(tmp);
        if (units != n) mim_panic("¨ (each): the function returned a value for some elements and nothing for others");
        return NULL;
    }
    mim_val *r = list_take(n, tmp);
    free(tmp);
    return r;
}

/* `xs ¨ f` where f takes its element over (it consumes it: owned.rs) and
 * xs is handed over too. A list nothing else holds gives its elements away
 * (f can update them in place) and its block is freed without them;
 * otherwise each element is handed over as a new reference. */
static void each_own_range(void *vc, int64_t lo, int64_t hi) {
    each_ctx *c = (each_ctx *)vc;
    mim_val **h = (mim_val **)c->xs->data;
    for (int64_t i = lo; i < hi; i++) c->out[i] = c->f(h[i]);
}

static void each_lend_range(void *vc, int64_t lo, int64_t hi) {
    each_ctx *c = (each_ctx *)vc;
    for (int64_t i = lo; i < hi; i++) c->out[i] = c->f(scalar_at(c->xs, i));
}

mim_val *mim_each_own(mim_val *xs, mim_fn1 f, int32_t pure) {
    need(xs, "¨ (each)");
    if (xs->rank == 0) return f(xs);
    int64_t n = xs->len;
    if (n == 0) {
        mim_release(xs);
        return alloc(MIM_I64, 1, 0);
    }
    int take = xs->dtype == MIM_BOXED && xs->rank == 1 && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1;
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    each_ctx c = {xs, f, tmp};
    mim_task run = take ? each_own_range : each_lend_range;
    if (pure == 2 && n > 1 && run_on_loop(n, run, &c)) {
    } else if (pure) parallel_for_grain(n, fine_grain(n), run, &c);
    else run(&c, 0, n);
    if (take) {
        free_block(&t_heap, xs); /* the block only: its elements were given away */
    } else {
        mim_release(xs);
    }
    int64_t units = 0;
    for (int64_t i = 0; i < n; i++) units += tmp[i] == NULL;
    if (units) {
        for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
        free(tmp);
        if (units != n) mim_panic("¨ (each): the function returned a value for some elements and nothing for others");
        return NULL;
    }
    mim_val *r = list_take(n, tmp);
    free(tmp);
    return r;
}

typedef struct {
    mim_val *xs;
    mim_val *f;
    mim_val **out;
    int64_t base; /* the ranges count from here */
} each_fn_ctx;

static void each_fn_range(void *vc, int64_t lo, int64_t hi) {
    each_fn_ctx *c = (each_fn_ctx *)vc;
    lo += c->base;
    hi += c->base;
    for (int64_t i = lo; i < hi; i++) {
        mim_val *e = scalar_at(c->xs, i);
        mim_val *r = mim_call(c->f, 1, &e);
        mim_release(e);
        c->out[i] = r;
    }
}

/* `xs ¨ f` with a function value: parallel when the function is pure. */
mim_val *mim_each_fn(mim_val *xs, mim_val *f) {
    need(xs, "¨ (each)");
    if (xs->rank == 0) return mim_call(f, 1, &xs);
    int64_t n = xs->len;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val **tmp = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    each_fn_ctx c = {xs, f, tmp, 0};
    if (f->dtype == MIM_FN && FN_DESC(f)->pure == 2 && n > 1 && run_on_loop(n, each_fn_range, &c)) {
        /* effects that may overlap (waiting): a task per element */
    } else if (f->dtype == MIM_FN && FN_DESC(f)->pure && n > 1 && g_nthreads > 1 && !t_in_parallel) {
        /* Waking the threads costs tens of microseconds: time the first
         * element, and share out the rest only when it is worth that. */
        struct timespec a, b;
        clock_gettime(CLOCK_MONOTONIC, &a);
        each_fn_range(&c, 0, 1);
        clock_gettime(CLOCK_MONOTONIC, &b);
        int64_t one = (int64_t)(b.tv_sec - a.tv_sec) * 1000000000 + (b.tv_nsec - a.tv_nsec);
        c.base = 1;
        if (one * (n - 1) > 50000) parallel_for_grain(n - 1, fine_grain(n - 1), each_fn_range, &c);
        else each_fn_range(&c, 0, n - 1);
    } else if (f->dtype == MIM_FN && FN_DESC(f)->pure) {
        parallel_for_grain(n, fine_grain(n), each_fn_range, &c);
    } else {
        each_fn_range(&c, 0, n);
    }
    /* a function run for its effects only: `xs ¨ (x => chan::close(x))` is () */
    int64_t units = 0;
    for (int64_t i = 0; i < n; i++) units += tmp[i] == NULL;
    if (units) {
        for (int64_t i = 0; i < n; i++) mim_release(tmp[i]);
        free(tmp);
        if (units != n) mim_panic("¨ (each): the function returned a value for some elements and nothing for others");
        return NULL;
    }
    mim_val *r = list_take(n, tmp); /* the results move in */
    free(tmp);
    if (xs->rank >= 2) set_shape(r, xs->rank, SHAPE(xs)); /* ¨ keeps the shape */
    return r;
}

/* ==== loop words: gather, trail, evolve, settle ========================== */

int64_t mim_truthy(mim_val *v);

static mim_val *call1(mim_val *f, mim_val *x) { return mim_call(f, 1, &x); }

static mim_val *call2(mim_val *f, mim_val *a, mim_val *b) {
    mim_val *args[2] = {a, b};
    return mim_call(f, 2, args);
}

static int fn_pure(const mim_val *f) { return f && f->dtype == MIM_FN && FN_DESC(f)->pure; }

/* Fold [lo, hi) of xs with f, left to right. */
static mim_val *fold_range(mim_val *xs, mim_val *f, int64_t lo, int64_t hi) {
    mim_val *acc = scalar_at(xs, lo);
    for (int64_t i = lo + 1; i < hi; i++) {
        mim_val *e = scalar_at(xs, i);
        mim_val *r = call2(f, acc, e);
        mim_release(acc);
        mim_release(e);
        acc = r;
    }
    return acc;
}

typedef struct {
    mim_val *xs, *f;
    int64_t n, chunk;
    mim_val **part;
} gather_ctx;

static void gather_chunks(void *vc, int64_t lo, int64_t hi) {
    gather_ctx *c = (gather_ctx *)vc;
    for (int64_t k = lo; k < hi; k++) {
        int64_t a = k * c->chunk, b = a + c->chunk < c->n ? a + c->chunk : c->n;
        c->part[k] = fold_range(c->xs, c->f, a, b);
    }
}

static int64_t loop_chunks(int64_t n) {
    int64_t k = (int64_t)g_nthreads * 4;
    return k < n ? k : n;
}

/* xs ▷ gather(f): combine every element with f. A pure f is applied as a
 * parallel tree (chunks fold on all cores, partial results combine in
 * order), which gives the same result as a left fold when f is associative. */
static mim_val *num_scan(mim_val *xs, mim_val *f, int scan);

mim_val *mim_gather(mim_val *xs, mim_val *f) {
    need(xs, "gather");
    if (xs->rank >= 2) mim_panic("gather works on lists; for a matrix reduce its rows with +/ ×/ ⌈/ ⌊/, or reshape it with ⥊");
    if (xs->rank == 0) {
        mim_retain(xs);
        return xs;
    }
    int64_t n = xs->len;
    if (n == 0) mim_panic("gather of an empty array: there is no first value to start from");
    mim_val *fast = num_scan(xs, f, 0);
    if (fast) return fast;
    if (!fn_pure(f) || g_nthreads < 2 || n < 64) return fold_range(xs, f, 0, n);
    int64_t nch = loop_chunks(n);
    gather_ctx c = {xs, f, n, (n + nch - 1) / nch, NULL};
    nch = (n + c.chunk - 1) / c.chunk;
    c.part = (mim_val **)malloc(sizeof(mim_val *) * (size_t)nch);
    parallel_for_grain(nch, 1, gather_chunks, &c);
    mim_val *acc = c.part[0];
    for (int64_t k = 1; k < nch; k++) {
        mim_val *r = call2(f, acc, c.part[k]);
        mim_release(acc);
        mim_release(c.part[k]);
        acc = r;
    }
    free(c.part);
    return acc;
}

/* ---- trail and gather of + × max min over numbers: no boxing ----------------
 * A fixed chunk size (not the thread count) decides how the work is split,
 * so float results are the same on any number of threads. */

enum { SOP_NONE, SOP_ADD, SOP_MUL, SOP_MAX, SOP_MIN };

static int fast_op(const mim_val *f) {
    if (!f || f->dtype != MIM_FN || f->len != 0) return SOP_NONE; /* no captures */
    const char *n = FN_DESC(f)->name;
    if (!n) return SOP_NONE;
    if (!strcmp(n, "(Add)")) return SOP_ADD;
    if (!strcmp(n, "(Mul)")) return SOP_MUL;
    if (!strcmp(n, "(Max)")) return SOP_MAX;
    if (!strcmp(n, "(Min)")) return SOP_MIN;
    return SOP_NONE;
}

#define SCAN_CHUNK ((int64_t)1 << 16)

#define COMBINE(op, a, b) ((op) == SOP_ADD ? (a) + (b) : (op) == SOP_MUL ? (a) * (b) : (op) == SOP_MAX ? ((b) > (a) ? (b) : (a)) : ((b) < (a) ? (b) : (a)))

typedef struct {
    const void *src;
    void *dst; /* NULL: reduce only */
    int64_t n;
    int op, isf;
    void *last; /* per chunk: its own combined value */
    void *offset; /* per chunk: what all earlier chunks combine to */
} nscan_ctx;

static void nscan_local(void *vc, int64_t lo, int64_t hi) {
    nscan_ctx *c = (nscan_ctx *)vc;
    for (int64_t k = lo; k < hi; k++) {
        int64_t a = k * SCAN_CHUNK, b = a + SCAN_CHUNK < c->n ? a + SCAN_CHUNK : c->n;
        if (c->isf) {
            const double *x = (const double *)c->src;
            double *d = (double *)c->dst, acc = x[a];
            if (d) d[a] = acc;
            for (int64_t i = a + 1; i < b; i++) {
                acc = COMBINE(c->op, acc, x[i]);
                if (d) d[i] = acc;
            }
            ((double *)c->last)[k] = acc;
        } else {
            const int64_t *x = (const int64_t *)c->src;
            int64_t *d = (int64_t *)c->dst, acc = x[a];
            if (d) d[a] = acc;
            for (int64_t i = a + 1; i < b; i++) {
                /* wrapping, as Mimic's integer arithmetic */
                acc = c->op == SOP_ADD ? (int64_t)((uint64_t)acc + (uint64_t)x[i]) : c->op == SOP_MUL ? (int64_t)((uint64_t)acc * (uint64_t)x[i]) : COMBINE(c->op, acc, x[i]);
                if (d) d[i] = acc;
            }
            ((int64_t *)c->last)[k] = acc;
        }
    }
}

static void nscan_fix(void *vc, int64_t lo, int64_t hi) {
    nscan_ctx *c = (nscan_ctx *)vc;
    for (int64_t k = lo < 1 ? 1 : lo; k < hi; k++) {
        int64_t a = k * SCAN_CHUNK, b = a + SCAN_CHUNK < c->n ? a + SCAN_CHUNK : c->n;
        if (c->isf) {
            double off = ((double *)c->offset)[k], *d = (double *)c->dst;
            for (int64_t i = a; i < b; i++) d[i] = COMBINE(c->op, off, d[i]);
        } else {
            int64_t off = ((int64_t *)c->offset)[k], *d = (int64_t *)c->dst;
            for (int64_t i = a; i < b; i++)
                d[i] = c->op == SOP_ADD ? (int64_t)((uint64_t)off + (uint64_t)d[i]) : c->op == SOP_MUL ? (int64_t)((uint64_t)off * (uint64_t)d[i]) : COMBINE(c->op, off, d[i]);
        }
    }
}

/* trail (scan = 1) or gather (scan = 0) of a non-empty number vector with
 * a fast operator; NULL when this does not apply */
static mim_val *num_scan(mim_val *xs, mim_val *f, int scan) {
    int op = fast_op(f);
    if (op == SOP_NONE || xs->rank != 1 || xs->len == 0 || (xs->dtype != MIM_I64 && xs->dtype != MIM_F64)) return NULL;
    int64_t n = xs->len, nch = (n + SCAN_CHUNK - 1) / SCAN_CHUNK;
    nscan_ctx c = {xs->data, NULL, n, op, xs->dtype == MIM_F64, malloc(8 * (size_t)nch), malloc(8 * (size_t)nch)};
    mim_val *r = NULL;
    if (scan) {
        r = alloc(xs->dtype, 1, n);
        c.dst = r->data;
    }
    parallel_for_grain(nch, 1, nscan_local, &c);
    /* combine the chunks in order */
    if (c.isf) {
        double *last = (double *)c.last, *off = (double *)c.offset;
        for (int64_t k = 1; k < nch; k++) off[k] = k == 1 ? last[0] : COMBINE(op, off[k - 1], last[k - 1]);
        if (!scan) r = mim_float(nch == 1 ? last[0] : COMBINE(op, off[nch - 1], last[nch - 1]));
    } else {
        int64_t *last = (int64_t *)c.last, *off = (int64_t *)c.offset;
        for (int64_t k = 1; k < nch; k++)
            off[k] = k == 1 ? last[0]
                            : op == SOP_ADD ? (int64_t)((uint64_t)off[k - 1] + (uint64_t)last[k - 1])
                            : op == SOP_MUL ? (int64_t)((uint64_t)off[k - 1] * (uint64_t)last[k - 1])
                                           : COMBINE(op, off[k - 1], last[k - 1]);
        if (!scan) {
            int64_t t = nch == 1 ? last[0]
                                 : op == SOP_ADD ? (int64_t)((uint64_t)off[nch - 1] + (uint64_t)last[nch - 1])
                                 : op == SOP_MUL ? (int64_t)((uint64_t)off[nch - 1] * (uint64_t)last[nch - 1])
                                                : COMBINE(op, off[nch - 1], last[nch - 1]);
            r = mim_int(t);
        }
    }
    if (scan && nch > 1) parallel_for_grain(nch, 1, nscan_fix, &c);
    free(c.last);
    free(c.offset);
    return r;
}

typedef struct {
    mim_val *xs, *f;
    int64_t n, chunk;
    mim_val **out;
    mim_val **offset; /* per chunk: the combined value of all earlier chunks */
} trail_ctx;

static void trail_local(void *vc, int64_t lo, int64_t hi) {
    trail_ctx *c = (trail_ctx *)vc;
    for (int64_t k = lo; k < hi; k++) {
        int64_t a = k * c->chunk, b = a + c->chunk < c->n ? a + c->chunk : c->n;
        c->out[a] = scalar_at(c->xs, a);
        for (int64_t i = a + 1; i < b; i++) {
            mim_val *e = scalar_at(c->xs, i);
            c->out[i] = call2(c->f, c->out[i - 1], e);
            mim_release(e);
        }
    }
}

static void trail_fix(void *vc, int64_t lo, int64_t hi) {
    trail_ctx *c = (trail_ctx *)vc;
    for (int64_t k = lo; k < hi; k++) {
        if (k == 0) continue;
        int64_t a = k * c->chunk, b = a + c->chunk < c->n ? a + c->chunk : c->n;
        for (int64_t i = a; i < b; i++) {
            mim_val *r = call2(c->f, c->offset[k], c->out[i]);
            mim_release(c->out[i]);
            c->out[i] = r;
        }
    }
}

/* xs ▷ trail(f): every running result [x0, f(x0,x1), f(f(x0,x1),x2), …].
 * A pure f runs as a two-pass parallel prefix (f associative). */
mim_val *mim_trail(mim_val *xs, mim_val *f) {
    need(xs, "trail");
    if (xs->rank >= 2) mim_panic("trail works on lists; for a matrix reduce its rows with +/ ×/ ⌈/ ⌊/, or reshape it with ⥊");
    if (xs->rank == 0) {
        mim_retain(xs);
        return xs;
    }
    int64_t n = xs->len;
    if (n == 0) return alloc(MIM_I64, 1, 0);
    mim_val *fast = num_scan(xs, f, 1);
    if (fast) return fast;
    trail_ctx c = {xs, f, n, n, NULL, NULL};
    c.out = (mim_val **)malloc(sizeof(mim_val *) * (size_t)n);
    if (!fn_pure(f) || g_nthreads < 2 || n < 64) {
        trail_local(&c, 0, 1);
    } else {
        int64_t nch = loop_chunks(n);
        c.chunk = (n + nch - 1) / nch;
        nch = (n + c.chunk - 1) / c.chunk;
        parallel_for_grain(nch, 1, trail_local, &c);
        /* offsets: what all earlier chunks combine to (sequential, nch values) */
        c.offset = (mim_val **)calloc((size_t)nch, sizeof(mim_val *));
        for (int64_t k = 1; k < nch; k++) {
            mim_val *last = c.out[k * c.chunk - 1];
            if (k == 1) {
                mim_retain(last);
                c.offset[1] = last;
            } else {
                c.offset[k] = call2(f, c.offset[k - 1], last);
            }
        }
        parallel_for_grain(nch, 1, trail_fix, &c);
        for (int64_t k = 1; k < nch; k++) mim_release(c.offset[k]);
        free(c.offset);
    }
    mim_val *r = mim_array_lit(n, c.out);
    for (int64_t i = 0; i < n; i++) mim_release(c.out[i]);
    free(c.out);
    return r;
}

/* x ▷ evolve(n, step): apply step n times. */
mim_val *mim_evolve(mim_val *x, int64_t n, mim_val *step) {
    if (n < 0) mim_panic("evolve: the number of steps cannot be negative");
    mim_retain(x);
    for (int64_t i = 0; i < n; i++) {
        mim_val *nx = call1(step, x);
        mim_release(x);
        x = nx;
    }
    return x;
}

/* x ▷ settle(step, done): apply step until done(x) holds (checked first). */
mim_val *mim_settle(mim_val *x, mim_val *step, mim_val *done) {
    mim_retain(x);
    for (;;) {
        mim_val *d = call1(done, x);
        int64_t stop = mim_truthy(d);
        mim_release(d);
        if (stop) return x;
        mim_val *nx = call1(step, x);
        mim_release(x);
        x = nx;
    }
}

/* ==== indexing =========================================================== */

static int64_t check_index(mim_val *xs, mim_val *i) {
    need(xs, "index");
    need(i, "index");
    if (xs->rank == 0) mim_panic("cannot index a scalar");
    if (i->rank != 0 || IS_FLOAT(i->dtype)) mim_panic("index must be an integer scalar");
    int64_t k = geti(i, 0);
    if (k < 0 || k >= xs->len) {
        char buf[128];
        snprintf(buf, sizeof buf, "index %lld out of bounds for length %lld", (long long)k, (long long)xs->len);
        mim_panic(buf);
    }
    return k;
}

static mim_val *map_get(mim_val *m, mim_val *k);
static void map_set(mim_val **slot, mim_val *k, mim_val *v);

/* an unshared copy with the same shape */
static mim_val *copy_whole(mim_val *a) {
    mim_val *r = alloc_like(a->dtype, a);
    memcpy(r->data, a->data, (size_t)a->len * elsize(a->dtype));
    retain_children(r);
    return r;
}

/* a scalar index into the leading axis */
static int64_t check_index_dim(mim_val *xs, mim_val *i) {
    need(i, "index");
    if (i->rank != 0 || IS_FLOAT(i->dtype) || IS_HANDLES(i)) mim_panic("index must be an integer scalar");
    int64_t k = geti(i, 0), d = dim_of(xs, 0);
    if (k < 0 || k >= d) {
        char buf[128];
        snprintf(buf, sizeof buf, "index %lld out of bounds for %lld rows", (long long)k, (long long)d);
        mim_panic(buf);
    }
    return k;
}

/* `m[i, j, …]`: the index is a tuple. Returns how many indices it holds and
 * the flat offset of the cell they select (`*cell` elements long). */
static int is_tuple(const mim_val *v) {
    return v && v->dtype == MIM_OBJ && v->rank == 0 && v->desc && v->desc->variant && v->desc->variant[0] == 0 &&
           strcmp(v->desc->type_name, "tuple") == 0;
}

static int64_t multi_offset(mim_val *xs, mim_val *t, int *nidx, int64_t *cell) {
    need(xs, "index");
    int n = (int)t->len;
    if (xs->rank == 0) mim_panic("cannot index a scalar");
    if (n > xs->rank) {
        char buf[128];
        snprintf(buf, sizeof buf, "%d indices for an array with %d dimension(s)", n, xs->rank);
        mim_panic(buf);
    }
    mim_val **h = (mim_val **)t->data;
    int64_t off = 0, stride = xs->len;
    for (int k = 0; k < n; k++) {
        mim_val *ik = h[k];
        if (ik->rank != 0 || IS_FLOAT(ik->dtype) || IS_HANDLES(ik)) mim_panic("index must be an integer scalar");
        int64_t d = dim_of(xs, k), v = geti(ik, 0);
        if (v < 0 || v >= d) {
            char buf[160];
            snprintf(buf, sizeof buf, "index %lld out of bounds for dimension %d of length %lld", (long long)v, k, (long long)d);
            mim_panic(buf);
        }
        stride /= d ? d : 1;
        off += v * stride;
    }
    *nidx = n;
    *cell = stride;
    return off;
}

/* compiled `xs[i]` on a vector of numbers that is not in bounds (or not
 * such a vector): the error, as mim_index would give it; never returns */
void mim_index_fail(mim_val *xs, int64_t i) {
    need(xs, "index");
    if (xs->rank == 1 && (i < 0 || i >= xs->len)) {
        char buf[128];
        snprintf(buf, sizeof buf, "index %lld out of bounds for length %lld", (long long)i, (long long)xs->len);
        mim_panic(buf);
    }
    char buf[160];
    snprintf(buf, sizeof buf, "index: expected a list of numbers, found %s", dtname(xs));
    mim_panic(buf);
}

/* A list stored with element type `dtype` (I64, F64 or BOOL): a value's
 * type says how it is stored, so a list of integers written where floats
 * are expected becomes floats here, once, not at every read. Returns a new
 * reference; `v` stays the caller's. Anything that cannot be converted is
 * returned as it is, for the usual error at its use. */
mim_val *mim_vec_as(mim_val *v, int32_t dtype) {
    /* f32 and i32 lists: from any list of numbers (rounded to f32; i32
     * stops the program on a value it cannot hold, or a fraction) */
    if (v && v->rank >= 1 && (dtype == MIM_F32 || dtype == MIM_I32) && v->dtype != dtype && !IS_HANDLES(v) && v->dtype != MIM_CHAR) {
        int64_t n = v->len;
        mim_val *r = alloc(dtype, v->rank, n);
        if (v->rank >= 2) set_shape(r, v->rank, SHAPE(v));
        for (int64_t k = 0; k < n; k++) {
            if (dtype == MIM_F32) {
                ((float *)r->data)[k] = (float)getf(v, k);
            } else {
                double x = getf(v, k);
                if (x != (double)(int32_t)x) {
                    mim_release(r);
                    char buf[128];
                    snprintf(buf, sizeof buf, "an i32 cannot hold %.17g", x);
                    mim_panic(buf);
                }
                ((int32_t *)r->data)[k] = (int32_t)x;
            }
        }
        return r;
    }
    if (!v || v->dtype == dtype || v->rank == 0 || (v->dtype != MIM_I64 && v->dtype != MIM_BOOL && v->dtype != MIM_I32 && v->dtype != MIM_F32)) {
        mim_retain(v);
        return v;
    }
    if (dtype != MIM_F64 && !(dtype == MIM_I64 && (v->dtype == MIM_BOOL || v->dtype == MIM_I32))) {
        mim_retain(v);
        return v;
    }
    int64_t n = v->len;
    mim_val *r = alloc(dtype, v->rank, n);
    if (v->rank >= 2) set_shape(r, v->rank, SHAPE(v));
    for (int64_t k = 0; k < n; k++) {
        if (dtype == MIM_F64) ((double *)r->data)[k] = getf(v, k);
        else ((int64_t *)r->data)[k] = geti(v, k);
    }
    return r;
}

/* A scalar as f32 or i32 (a `let x: f32 = …` or an f32 field): rounded to
 * f32; i32 stops the program on a value it cannot hold. */
mim_val *mim_scalar_as(mim_val *v, int32_t dtype) {
    need(v, "a number");
    if (v->rank != 0 || IS_HANDLES(v) || v->dtype == MIM_CHAR) {
        char buf[128];
        snprintf(buf, sizeof buf, "expected a number, found %s", dtname(v));
        mim_panic(buf);
    }
    if (v->dtype == dtype) {
        mim_retain(v);
        return v;
    }
    if (dtype == MIM_F32) return mim_float32(getf(v, 0));
    double x = getf(v, 0);
    if (x != (double)(int32_t)x) {
        char buf[128];
        snprintf(buf, sizeof buf, "an i32 cannot hold %.17g", x);
        mim_panic(buf);
    }
    return mim_int32((int64_t)x);
}

/* A vector of `n` elements stored as `dtype`, for a list the compiler keeps
 * in registers (codegen as_fixed): a new reference, converted from integers
 * or truth values when needed; anything else stops the program. */
mim_val *mim_fixed_in(mim_val *v, int32_t dtype, int64_t n, const char *what) {
    if (v && v->rank == 1 && v->len == n && v->dtype == dtype) {
        mim_retain(v);
        return v;
    }
    mim_val *r = mim_vec_as(v, dtype);
    if (r && r->rank == 1 && r->len == n && r->dtype == dtype) return r;
    char buf[200];
    snprintf(buf, sizeof buf, "%s: expected a list of %lld %s, found %s", what, (long long)n,
             dtype == MIM_F64 ? "floats" : dtype == MIM_BOOL ? "truth values" : "integers", r ? dtname(r) : "nothing");
    mim_panic(buf);
    return NULL;
}

mim_val *mim_index(mim_val *xs, mim_val *i) {
    if (xs && xs->dtype == MIM_MAP) return map_get(xs, i);
    if (is_tuple(i)) {
        int n;
        int64_t cell, off = multi_offset(xs, i, &n, &cell);
        if (n == xs->rank) return scalar_at(xs, off);
        /* fewer indices than dimensions: the sub-array they select */
        mim_val *r = alloc_shaped(xs->dtype, xs->rank - n, SHAPE(xs) + n);
        size_t es = elsize(xs->dtype);
        memcpy(r->data, (char *)xs->data + (size_t)off * es, (size_t)cell * es);
        retain_children(r);
        return r;
    }
    if (xs && xs->rank >= 2) {
        int64_t k = check_index_dim(xs, i);
        return cell_at(xs, k);
    }
    return scalar_at(xs, check_index(xs, i));
}

/* ==== pattern matching helpers ========================================== */

/* Is `o` a record built from descriptor `d`? */
/* A record of the type `d` describes, read field by field into registers
 * by compiled code (codegen as_flat); anything else stops the program. */
void mim_check_record(mim_val *o, const mim_desc *d, const char *what) {
    if (o && o->dtype == MIM_OBJ && o->rank == 0 && o->desc == d) return;
    char buf[200];
    snprintf(buf, sizeof buf, "%s: expected a %s record, found %s", what, d && d->type_name ? d->type_name : "", o ? dtname(o) : "nothing");
    mim_panic(buf);
}

int64_t mim_pat_is(mim_val *o, const mim_desc *d) { return o && o->dtype == MIM_OBJ && o->rank == 0 && o->desc == d; }

/* Length of a vector (an array or string), or -1 for anything else. */
int64_t mim_pat_len(mim_val *v) {
    if (!v || v->rank != 1 || v->dtype == MIM_MAP) return -1;
    return v->len;
}

/* Element `i` of a vector already known to be long enough (owned). */
mim_val *mim_pat_at(mim_val *v, int64_t i) { return scalar_at(v, i); }

/* The elements from `start` on (owned). */
mim_val *mim_pat_rest(mim_val *v, int64_t start) { return slice(v, start, v->len - start); }

/* Copy-on-write element store: mutates in place when uniquely owned. */
void mim_set_index(mim_val **slot, mim_val *i, mim_val *v) {
    if (*slot && (*slot)->dtype == MIM_MAP) {
        map_set(slot, i, v);
        return;
    }
    mim_val *xs = *slot;
    int64_t k;
    if (is_tuple(i)) {
        int n;
        int64_t cell;
        k = multi_offset(xs, i, &n, &cell);
        if (n != xs->rank) mim_panic("indexed assignment needs one index per dimension: m[i, j] = v");
    } else {
        if (xs && xs->rank >= 2) mim_panic("indexed assignment on a matrix needs one index per dimension: m[i, j] = v");
        k = check_index(xs, i);
    }
    need(v, "indexed assignment");
    if (xs->dtype == MIM_BOXED) { /* a list of handles: replace one (copy-on-write) */
        if (__atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) > 1) {
            mim_val *c = copy_whole(xs);
            mim_release(xs);
            *slot = xs = c;
        }
        mim_val **h = (mim_val **)xs->data;
        mim_retain(v);
        mim_release(h[k]);
        h[k] = v;
        return;
    }
    if (v->rank != 0 || IS_HANDLES(v)) mim_panic("indexed assignment: a numeric array can only hold scalars");
    if (xs->dtype == MIM_U8) { /* bytes stay bytes: 0 … 255 */
        if (v->dtype == MIM_F64 || geti(v, 0) < 0 || geti(v, 0) > 255) mim_panic("indexed assignment: a byte is an integer from 0 to 255");
        if (__atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) > 1) {
            mim_val *c = copy_whole(xs);
            mim_release(xs);
            *slot = xs = c;
        }
        ((uint8_t *)xs->data)[k] = (uint8_t)geti(v, 0);
        return;
    }
    int newdt = xs->dtype;
    if (IS_FLOAT(v->dtype) && !IS_FLOAT(xs->dtype)) newdt = MIM_F64;
    if (__atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) > 1 || newdt != xs->dtype) {
        mim_val *c = alloc_like(newdt, xs);
        for (int64_t j = 0; j < xs->len; j++) {
            if (IS_FLOAT(newdt)) setf(c, j, getf(xs, j));
            else seti(c, j, geti(xs, j));
        }
        mim_release(xs);
        *slot = xs = c;
    }
    if (IS_FLOAT(xs->dtype)) setf(xs, k, getf(v, 0));
    else seti(xs, k, geti(v, 0));
}

int64_t mim_par_grain(void) { return g_grain; }

/* Length of a vector a fused kernel reads directly (its element type must be
 * exactly the one the compiler inferred). */
int64_t mim_vec_len(mim_val *v, int32_t dtype) {
    need(v, "array operand");
    if (v->rank != 1 || v->dtype != dtype) {
        char buf[160];
        snprintf(buf, sizeof buf, "internal error: fused kernel expected a vector of dtype %d, found %s", dtype, dtname(v));
        mim_panic(buf);
    }
    return v->len;
}

int64_t mim_truthy(mim_val *v) {
    need(v, "condition");
    if (IS_HANDLES(v)) mim_panic("condition must be a number or bool, found a record or list");
    if (v->rank != 0) mim_panic("condition must be a scalar, found an array (reduce it first, e.g. with ⌈/)");
    return getf(v, 0) != 0;
}

static int type_ok(mim_val *v, int32_t ty);

/* Non-panicking test, used by compiled dispatch thunks. */
int32_t mim_is_type(mim_val *v, int32_t ty) { return type_ok(v, ty); }

void mim_check_type(mim_val *v, int32_t ty, const char *what) {
    if (type_ok(v, ty)) return;
    static const char *names[] = {"", "integer scalar", "f64 scalar", "bool", "char", "str", "[i64]", "[f64]", "[bool]", "function", "[u8]", "f32 scalar", "i32 scalar", "[f32]", "[i32]"};
    char buf[256];
    snprintf(buf, sizeof buf, "type error: %s expected %s, found %s", what, names[ty], dtname(v));
    mim_panic(buf);
}

static int type_ok(mim_val *v, int32_t ty) {
    int ok = 0;
    if (v) {
        int s = v->rank == 0;
        switch (ty) {
        case TY_INT: ok = s && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL || v->dtype == MIM_I32); break;
        case TY_FLOAT: ok = s && (v->dtype == MIM_F64 || v->dtype == MIM_I64 || v->dtype == MIM_BOOL || v->dtype == MIM_F32 || v->dtype == MIM_I32); break;
        case TY_BOOL: ok = s && v->dtype == MIM_BOOL; break;
        case TY_CHAR: ok = s && v->dtype == MIM_CHAR; break;
        case TY_STR: ok = !s && v->dtype == MIM_CHAR; break;
        case TY_INTVEC: ok = !s && (v->dtype == MIM_I64 || v->dtype == MIM_BOOL); break;
        case TY_FLOATVEC: ok = !s && (v->dtype == MIM_F64 || v->dtype == MIM_I64 || v->dtype == MIM_BOOL); break;
        case TY_BOOLVEC: ok = !s && v->dtype == MIM_BOOL; break;
        case TY_FN: ok = v->dtype == MIM_FN; break;
        case TY_BYTES: ok = !s && v->dtype == MIM_U8; break;
        case TY_F32: ok = s && v->dtype == MIM_F32; break;
        case TY_I32: ok = s && v->dtype == MIM_I32; break;
        case TY_F32VEC: ok = !s && v->dtype == MIM_F32; break;
        case TY_I32VEC: ok = !s && v->dtype == MIM_I32; break;
        }
    }
    return ok;
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
/* standard-library intrinsics (strings, lists, maps) */
#include "mimic_std.c"
/* the event loop: coroutines for tasks that wait */
#include "mimic_sched.c"
/* program basics: arguments, environment, files, directories, paths */
#include "mimic_io.c"
/* the browser side of ui::run */
#include "mimic_ui.c"
/* networking: TCP, HTTP client and server */
#include "mimic_socket.c"
/* the json module */
#include "mimic_json.c"
/* native windows for ui (macOS) */
#include "mimic_window.c"
/* GPU kernels (Metal, macOS) */
#include "mimic_gpu.c"
