/* Lists and maps handed over by compiled code at their last use, changed in
 * place: `push`, `put` and `concat` of a value the compiled code owns there
 * (the compiler calls these instead of mim_std_push_own and the others).
 *
 * At the very end of the runtime: a function added earlier in it moved the
 * code of every function after it, which on some machines changed the speed
 * of programs that never call it. */

/* a list of values held by reference (strings, records, lists, …), held
 * once: its slots can change in place, each holding a reference of its own */
static int own_handles(const mim_val *xs) {
    return xs && xs->rank == 1 && xs->dtype == MIM_BOXED && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1;
}

/* push(xs, x), xs owned: a list of references held once grows in place
 * (before, a list of strings or records was copied at every push) */
mim_val *mim_take_push(mim_val *xs, mim_val *x) {
    if (own_handles(xs) && x && x != xs) {
        int64_t n = xs->len;
        xs = grow_owned(xs, n + 1);
        mim_retain(x);
        ((mim_val **)xs->data)[n] = x;
        return xs;
    }
    return mim_std_push_own(xs, x);
}

/* put(xs, i, v), xs owned: a list of references held once, or a map held
 * once (as `m[k] = v` changes one), changed in place */
mim_val *mim_take_put(mim_val *xs, mim_val *k, mim_val *v) {
    if (own_handles(xs) && v && v != xs && k && k->rank == 0 && (k->dtype == MIM_I64 || k->dtype == MIM_I32)) {
        int64_t i = geti(k, 0);
        if (i >= 0 && i < xs->len) {
            mim_val **slot = (mim_val **)xs->data + i;
            mim_val *old = *slot;
            mim_retain(v);
            *slot = v;
            mim_release(old);
            return xs;
        }
    }
    if (xs && xs->dtype == MIM_MAP && v != xs && __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1) {
        mim_val *slot = xs;
        mim_set_index(&slot, k, v);
        return slot;
    }
    return mim_std_put_own(xs, k, v);
}

/* concat(xs, ys), xs owned */
mim_val *mim_take_concat(mim_val *xs, mim_val *ys) {
    return mim_std_concat_own(xs, ys);
}

/* xs[i] = v on a variable's list: the list (owned) in, the list out (a
 * copy when it was shared), so the compiled code keeps the variable in a
 * register instead of handing the runtime its address */
mim_val *mim_set_index_v(mim_val *xs, mim_val *i, mim_val *v) {
    mim_val *slot = xs;
    mim_set_index(&slot, i, v);
    return slot;
}

/* ==== short `¨`s: on one core unless worth sharing ======================
 * (here at the end of the runtime: added in the middle of it, these moved
 * the scheduler's code, and `fib`, which never maps, measured 20% slower on
 * one machine) */

/* What a `¨`'s function has cost per element (ns), by the function: a
 * small table, written without locks (a torn or lost entry only means one
 * more timing). */
typedef struct { void *key; int64_t ns; } each_cost_slot;
/* allocated at the first short `¨` (as a 4 KB static array it moved every
 * global after it by a page, and `fib`, which never maps, measured 20%
 * slower on one core on one machine) */
static each_cost_slot *g_each_cost;

static int64_t each_cost(void *key) {
    each_cost_slot *t = __atomic_load_n(&g_each_cost, __ATOMIC_ACQUIRE);
    if (!t) return 0;
    unsigned i = (unsigned)(((uintptr_t)key >> 4) * 2654435761u) & 255;
    if (__atomic_load_n(&t[i].key, __ATOMIC_RELAXED) != key) return 0;
    return __atomic_load_n(&t[i].ns, __ATOMIC_RELAXED);
}

static void each_cost_put(void *key, int64_t ns) {
    each_cost_slot *t = __atomic_load_n(&g_each_cost, __ATOMIC_ACQUIRE);
    if (!t) {
        each_cost_slot *n = (each_cost_slot *)calloc(256, sizeof(each_cost_slot));
        if (!n) return;
        each_cost_slot *expected = NULL;
        if (__atomic_compare_exchange_n(&g_each_cost, &expected, n, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) t = n;
        else {
            free(n);
            t = expected;
        }
    }
    unsigned i = (unsigned)(((uintptr_t)key >> 4) * 2654435761u) & 255;
    __atomic_store_n(&t[i].ns, ns < 1 ? 1 : ns, __ATOMIC_RELAXED);
    __atomic_store_n(&t[i].key, key, __ATOMIC_RELAXED);
}

/* The first chunk (from element 0) of a `¨` shared out for the first time,
 * timed, whichever thread runs it: what the function costs per element.
 * (No thread-local flag: adding one moved the scheduler's thread-locals,
 * and `fib`, which never maps, measured 20% slower on one machine.) */
typedef struct {
    void (*f)(void *, int64_t, int64_t);
    void *ctx;
    int64_t ns; /* per element, once timed */
} probe_ctx;

static void probe_range(void *c, int64_t lo, int64_t hi) {
    probe_ctx *p = (probe_ctx *)c;
    if (lo != 0) {
        p->f(p->ctx, lo, hi);
        return;
    }
    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    p->f(p->ctx, lo, hi);
    clock_gettime(CLOCK_MONOTONIC, &b);
    int64_t spent = (int64_t)(b.tv_sec - a.tv_sec) * 1000000000 + (b.tv_nsec - a.tv_nsec);
    p->ns = hi > lo ? (spent / (hi - lo) > 0 ? spent / (hi - lo) : 1) : 0;
}

/* A pure `¨` of the function `key` over n elements. A long list goes in
 * fine chunks over every core, as always, and so does a short one the
 * first time (up to a chunk, 16,384), while its cost per element is timed.
 * After that a short list is shared out only when that is worth waking the
 * pool for at the cost seen, else it runs here, timed as it goes (1, 2, 4,
 * … elements, and shared out if it turns out costly after all). (Sent
 * straight to the pool every time, the 9-element `¨`s of a sudoku check
 * woke every thread 27,000 times in 20,000 grids, and ran 6x slower on 12
 * cores than on one. Timed before sharing every time instead, a few costly
 * elements, such as 25 searches done once, ran the first alone.) */
static void each_par(int64_t n, void (*range)(void *, int64_t, int64_t), void *ctx, void *key) {
    if (n > mim_grain || n == 1 || g_nthreads <= 1 || t_in_parallel) {
        parallel_for_grain(n, fine_grain(n), range, ctx);
        return;
    }
    int64_t per = each_cost(key);
    if (per == 0 || each_worth_sharing(per, 1, n)) {
        probe_ctx p = {range, ctx, 0};
        parallel_for_grain(n, fine_grain(n), probe_range, &p);
        if (p.ns) each_cost_put(key, p.ns);
        return;
    }
    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    int64_t done = 0, step = 1;
    while (done < n) {
        int64_t hi = done + step < n ? done + step : n;
        range(ctx, done, hi);
        done = hi;
        clock_gettime(CLOCK_MONOTONIC, &b);
        int64_t spent = (int64_t)(b.tv_sec - a.tv_sec) * 1000000000 + (b.tv_nsec - a.tv_nsec);
        if (done == n) {
            each_cost_put(key, spent / n);
            return;
        }
        if (each_worth_sharing(spent, done, n - done)) {
            each_cost_put(key, spent / done);
            timed_ctx c = {range, ctx, done};
            parallel_for_grain(n - done, fine_grain(n - done), timed_range, &c);
            return;
        }
        step *= 2;
    }
}


/* After timing the first `done` elements (`spent` ns): is the rest, `left`
 * elements at that pace, worth sharing out? Waking the pool costs tens of
 * microseconds, and more the more threads it wakes (each one it wakes takes
 * the lock): at least 50 us of work, and 20 us for each thread that would
 * get some. */
static int each_worth_sharing(int64_t spent, int64_t done, int64_t left) {
    int64_t rest = spent / done * left;
    int64_t k = left < g_nthreads ? left : g_nthreads;
    return rest > 50000 && rest > 20000 * k;
}

/* ==== a crash said, and the output so far kept ===========================
 * A program that crashes (a bad memory access, a stack overflow) used to
 * lose what it had printed but not yet flushed, so the crash looked as if it
 * came earlier than it did. The handler flushes, names the signal, and
 * raises it again (the exit status stays the signal's). An alarm first: a
 * crash inside printf holds stdout's lock, and the flush must not hang. */
#if defined(_WIN32)
static LONG WINAPI crash_filter(EXCEPTION_POINTERS *e) {
    DWORD c = e && e->ExceptionRecord ? e->ExceptionRecord->ExceptionCode : 0;
    fflush(stdout);
    const char *what = c == EXCEPTION_ACCESS_VIOLATION ? "an access violation (a bad memory access)"
                     : c == EXCEPTION_STACK_OVERFLOW   ? "a stack overflow"
                                                       : "an exception";
    fprintf(stderr, "mimic: the program crashed: %s (0x%08lX)\n", what, (unsigned long)c);
    fflush(stderr);
    return EXCEPTION_CONTINUE_SEARCH;
}
static void mim_crash_handlers(void) { SetUnhandledExceptionFilter(crash_filter); }
#elif !defined(__wasi__)
static void crash_signal(int sig) {
    alarm(2);
    fflush(stdout);
    const char *m = sig == SIGSEGV ? "mimic: the program crashed: SIGSEGV (a bad memory access, or the stack ran out)\n"
                  : sig == SIGBUS  ? "mimic: the program crashed: SIGBUS (a bad memory access)\n"
                  : sig == SIGILL  ? "mimic: the program crashed: SIGILL (an illegal instruction)\n"
                                   : "mimic: the program crashed: SIGFPE (an arithmetic fault)\n";
    ssize_t w = write(2, m, strlen(m));
    (void)w;
    signal(sig, SIG_DFL);
    raise(sig);
}
#if defined(__APPLE__)
#include <TargetConditionals.h>
#endif
static void mim_crash_handlers(void) {
    /* (on a stack of its own: a stack overflow is reported too; tvOS and
     * watchOS have no sigaltstack) */
#if !(defined(__APPLE__) && (TARGET_OS_TV || TARGET_OS_WATCH))
    static char *alt;
    if (!alt && (alt = (char *)malloc(64 * 1024))) {
        stack_t ss;
        memset(&ss, 0, sizeof ss);
        ss.ss_sp = alt;
        ss.ss_size = 64 * 1024;
        sigaltstack(&ss, NULL);
    }
#endif
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = crash_signal;
    sa.sa_flags = SA_ONSTACK;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGSEGV, &sa, NULL);
    sigaction(SIGBUS, &sa, NULL);
    sigaction(SIGILL, &sa, NULL);
    sigaction(SIGFPE, &sa, NULL);
}
#else
static void mim_crash_handlers(void) {}
#endif

/* ==== a list shrunk in place ==============================================
 * `xs ↑ k` and `xs ↓ k` of a list the compiled code owns at its last use:
 * held once, it keeps its block and loses its ends (a heap's pop, h ↑ (n -
 * 1), copied the heap every time). Only while its pool stays the same (or it
 * stays past the pools): free_block picks the pool by length. Else, and when
 * it is shared, as ↑ / ↓ always did, the list released. */
mim_val *mim_shrink_own(mim_val *xs, int64_t k, int32_t drop) {
    if (xs && xs->rank == 1 && xs->dtype != MIM_MAP && xs->dtype != MIM_OBJ && xs->dtype != MIM_FN &&
        __atomic_load_n(&xs->rc, __ATOMIC_ACQUIRE) == 1) {
        int64_t len = xs->len, n = k < 0 ? -k : k;
        if (n > len) n = len;
        int64_t keep = drop ? len - n : n;
        /* the first element kept */
        int64_t from = (!drop && k < 0) || (drop && k >= 0) ? len - keep : 0;
        int dt = xs->dtype;
        int c_old = size_class(payload_bytes(dt, len)), c_new = size_class(payload_bytes(dt, keep));
        if (c_old == c_new || (c_old >= SIZE_CLASSES && c_new >= SIZE_CLASSES)) {
            size_t es = elsize(dt);
            if (IS_HANDLES(xs)) {
                mim_val **h = (mim_val **)xs->data;
                for (int64_t i = 0; i < from; i++) mim_release(h[i]);
                for (int64_t i = from + keep; i < len; i++) mim_release(h[i]);
            }
            if (from) memmove(xs->data, (char *)xs->data + from * es, (size_t)keep * es);
            xs->len = keep;
            if (es == 1) ((char *)xs->data)[keep] = 0;
            return xs;
        }
    }
    mim_val *kv = mim_int(k);
    mim_val *r = mim_prim2(drop ? P_DROP : P_TAKE, xs, kv);
    mim_release(kv);
    mim_release(xs);
    return r;
}
/* ==== a float's bits, and floats as bytes ================================
 * (math::to_bits and the rest, bytes::of_f64s and bytes::to_f64s: weights
 * saved exactly, as 8 bytes each, little-endian on every target) */

static int64_t bits_int_arg(mim_val *x, const char *fn) {
    if (!x || x->rank != 0 || IS_FLOAT(x->dtype) || x->dtype == MIM_BOOL || IS_HANDLES(x)) {
        char buf[96];
        snprintf(buf, sizeof buf, "%s: expected an integer", fn);
        mim_panic(buf);
    }
    return geti(x, 0);
}

mim_val *mim_std_math_to_bits(mim_val *a) {
    double x = math_arg(a, "to_bits");
    uint64_t u;
    memcpy(&u, &x, 8);
    return mim_int((int64_t)u);
}

mim_val *mim_std_math_from_bits(mim_val *b) {
    uint64_t u = (uint64_t)bits_int_arg(b, "math::from_bits");
    double x;
    memcpy(&x, &u, 8);
    return mim_float(x);
}

mim_val *mim_std_math_to_bits32(mim_val *a) {
    float x = (float)math_arg(a, "to_bits32");
    uint32_t u;
    memcpy(&u, &x, 4);
    return mim_int((int64_t)u);
}

mim_val *mim_std_math_from_bits32(mim_val *b) {
    int64_t i = bits_int_arg(b, "math::from_bits32");
    if (i < 0 || i > 0xFFFFFFFFLL) mim_panic("math::from_bits32: the bits of an f32 are 0 … 4294967295");
    uint32_t u = (uint32_t)i;
    float x;
    memcpy(&x, &u, 4);
    return mim_float32(x);
}

mim_val *mim_std_bytes_of_f64s(mim_val *xs) {
    need(xs, "bytes::of_f64s");
    if (xs->rank != 1 || IS_HANDLES(xs) || (xs->len > 0 && !IS_FLOAT(xs->dtype) && xs->dtype != MIM_I64))
        mim_panic("bytes::of_f64s: expected a list of floats");
    mim_val *r = alloc(MIM_U8, 1, xs->len * 8);
    uint8_t *o = (uint8_t *)r->data;
    for (int64_t i = 0; i < xs->len; i++) {
        double x = getf(xs, i);
        uint64_t u;
        memcpy(&u, &x, 8);
        for (int k = 0; k < 8; k++) o[i * 8 + k] = (uint8_t)(u >> (8 * k));
    }
    return r;
}

mim_val *mim_std_bytes_to_f64s(mim_val *b) {
    need(b, "bytes::to_f64s");
    if (b->rank != 1 || (b->len > 0 && b->dtype != MIM_U8)) mim_panic("bytes::to_f64s: expected bytes ([u8])");
    if (b->len % 8) {
        char buf[128];
        snprintf(buf, sizeof buf, "bytes::to_f64s: %lld bytes is not a whole number of floats (8 bytes each)", (long long)b->len);
        mim_panic(buf);
    }
    mim_val *r = alloc(MIM_F64, 1, b->len / 8);
    const uint8_t *d = (const uint8_t *)b->data;
    for (int64_t i = 0; i < b->len / 8; i++) {
        uint64_t u = 0;
        for (int k = 0; k < 8; k++) u |= (uint64_t)d[i * 8 + k] << (8 * k);
        double x;
        memcpy(&x, &u, 8);
        ((double *)r->data)[i] = x;
    }
    return r;
}

/* An f64 as text: the shortest that reads back as the same number (15
 * significant digits where those do, as before; 16 or 17 where they do not:
 * "{x}" then parses back to x, as in Rust, Go, JavaScript and Python) */
static void fmt_f64(char *buf, size_t n, double x) {
    for (int prec = 15; prec <= 17; prec++) {
        snprintf(buf, n, "%.*g", prec, x);
        if (!isfinite(x) || strtod(buf, NULL) == x) return;
    }
}

/* ==== the memory check (debug builds) =====================================
 * free_block hands each freed value to memcheck_hold: its header is marked
 * (count MIM_FREED_RC; type MIM_FREED_DT, so no check of a type in compiled
 * code passes and every use reaches the runtime; length 0), its elements
 * filled with 0xDB, and it is kept in a queue (up to MC_KEEP values and
 * MC_BYTES bytes) before its memory goes back to be used again. A retain, a
 * release or a read of a marked value calls mim_memcheck_failed, which names
 * what was done and what the value was; `mimic lldb` stops there, with the
 * program's own stack above it. (A list freed before its last read went
 * unseen while its memory stayed as it was.) */
#ifdef MIM_MEMCHECK
#define MC_KEEP 65536
#define MC_BYTES ((size_t)64 << 20)
#define MC_BIG ((size_t)16 << 20)
typedef struct {
    mim_val *v;
    size_t bytes;
    int32_t dtype, rank;
    int64_t len;
    const mim_desc *desc;
} mc_entry;
static mc_entry *g_mc;
static int64_t g_mc_head, g_mc_n;
static size_t g_mc_bytes;
static int g_mc_lock;

static void mc_lock(void) {
    while (__atomic_test_and_set(&g_mc_lock, __ATOMIC_ACQUIRE)) {
    }
}
static void mc_unlock(void) { __atomic_clear(&g_mc_lock, __ATOMIC_RELEASE); }

/* (not static, never inlined: `mimic lldb` sets a breakpoint here) */
__attribute__((noinline)) void mim_memcheck_failed(const char *msg) {
    fflush(stdout);
    fprintf(stderr, "mimic panic: memory check: %s\n", msg);
    fprintf(stderr, "  (a debug build's check; `mimic lldb` stops at the place, with the stack)\n");
    exit(101);
}

static void memcheck_hit(const mim_val *v, const char *what) {
    mim_val was = {1, MIM_I64, 0, 0, NULL};
    int found = 0;
    mc_lock();
    for (int64_t k = 0; k < g_mc_n && !found; k++) {
        mc_entry *e = &g_mc[(g_mc_head + k) % MC_KEEP];
        if (e->v == v) {
            was.dtype = e->dtype;
            was.rank = e->rank;
            was.len = e->len;
            was.desc = e->dtype == MIM_OBJ ? e->desc : NULL;
            found = 1;
        }
    }
    mc_unlock();
    char buf[256];
    if (!found) snprintf(buf, sizeof buf, "a value was %s after it was freed", what);
    else if (was.rank == 0 && was.dtype != MIM_OBJ) snprintf(buf, sizeof buf, "a value (%s) was %s after it was freed", dtname(&was), what);
    else snprintf(buf, sizeof buf, "a value (%s, %lld element%s) was %s after it was freed", dtname(&was), (long long)was.len, was.len == 1 ? "" : "s", what);
    mim_memcheck_failed(buf);
}

static mim_val *memcheck_hold(mim_val *v, size_t bytes, size_t *out_bytes) {
    mc_entry e = {v, bytes, v->dtype, v->rank, v->len, v->desc};
    size_t size = sizeof(mim_val) + bytes;
    memset(v->data, 0xDB, bytes < MC_BIG ? bytes : MC_BIG);
    v->rc = MIM_FREED_RC;
    v->dtype = MIM_FREED_DT;
    v->rank = 1;
    v->len = 0;
    if (size > MC_BIG) {
        /* (a very large block is not kept: its memory goes back at once) */
        *out_bytes = bytes;
        return v;
    }
    mc_entry out = {0};
    mc_lock();
    if (!g_mc) g_mc = (mc_entry *)calloc(MC_KEEP, sizeof(mc_entry));
    if (!g_mc) {
        mc_unlock();
        *out_bytes = bytes;
        return v;
    }
    if (g_mc_n == MC_KEEP || g_mc_bytes + size > MC_BYTES) {
        out = g_mc[g_mc_head];
        g_mc_head = (g_mc_head + 1) % MC_KEEP;
        g_mc_n--;
        g_mc_bytes -= sizeof(mim_val) + out.bytes;
    }
    g_mc[(g_mc_head + g_mc_n) % MC_KEEP] = e;
    g_mc_n++;
    g_mc_bytes += size;
    mc_unlock();
    *out_bytes = out.bytes;
    return out.v;
}
#endif
