/* Mimic runtime — interaction-net layer (Milestone 2, phase 2).
 *
 * Included at the end of mimic_rt.c (same translation unit).
 *
 * A net is a graph of nodes wired port-to-port. Computation is the local
 * rewriting of "active pairs" (two principal ports facing each other). Every
 * rewrite touches only its pair, and the rewrite system is confluent, so any
 * number of threads may reduce disjoint pairs at once — that is the source of
 * Mimic's automatic parallelism.
 *
 *   tag  node   arity  meaning
 *   VAR  -      -      a wire (index into the var table)
 *   REF  @def   0      a not-yet-expanded global definition (lazy)
 *   ERA  *      0      eraser: deletes whatever it meets
 *   NUM  #n     0      number, unboxed in the port when it fits
 *   ARR  -      0      handle to an array-layer value (mim_val*)
 *   CON  (a b)  2      constructor: lambdas / application / pairs
 *   DUP  {a b}  2      duplicator (labelled)
 *   OPR  $op    2      operator: (2nd operand, result); kernels on arrays
 *   SWI  ?      2      switch: ((then else), result) — the untaken branch is erased
 *
 * Port layout (64 bit):  payload:60 | tag:4
 *   binary nodes:  payload = aux:28 << 32 | loc:32   (aux = DUP label / OPR op)
 *   NUM:           payload = value:58 << 2 | kind:2  (kind: int, boxed, bool, char)
 *   ARR:           payload = pointer
 *
 * Memory: nodes and vars live in two global tables; each thread allocates from
 * private chunks and keeps private free lists. Arrays held by ARR ports are
 * owned references: DUP retains, ERA releases. Nets are linear, so boxed
 * numbers need no refcount at all.
 *
 * Linking follows HVM2's lock-free algorithm: a wire's two ends race to an
 * atomic exchange on its var slot; the second to arrive connects both sides.
 */

typedef uint64_t Port;

enum { T_VAR = 0, T_REF, T_ERA, T_NUM, T_CON, T_DUP, T_OPR, T_SWI, T_ARR, T_LIT };
enum { NK_INT = 0, NK_BOX = 1, NK_BOOL = 2, NK_CHAR = 3 };

#define P_TAG(p) ((uint32_t)((p) & 0xF))
#define P_PAY(p) ((p) >> 4)
#define P_LOC(p) ((uint32_t)(P_PAY(p) & 0xFFFFFFFFu))
#define P_AUX(p) ((uint32_t)(P_PAY(p) >> 32))
#define MK(tag, pay) ((((uint64_t)(pay)) << 4) | (uint64_t)(tag))
#define MKN(tag, loc, aux) MK(tag, (((uint64_t)(aux)) << 32) | (uint64_t)(loc))
#define NONE (~(Port)0)
#define ERA_P MK(T_ERA, 0)

/* OPR aux: op | flip << 8. Ops 0..21 = binary (BinOp), 24..27 = neg/not/abs/bnot,
 * 32+p = monadic primitive p, 64+p = dyadic primitive p. */
#define OP_FLIP 0x100
#define OP_NEG 24
#define OP_NOT 25
#define OP_ABS 26
#define OP_BNOT 27
#define OP_PRIM1 32
#define OP_PRIM2 64
#define OP_INDEX 96
#define OP_CHECK 128 /* 128 + type code; message index in aux bits 12.. */

typedef struct { Port a, b; } Pair;

/* hot helpers must inline: each is a handful of instructions */
#define HOT static inline __attribute__((always_inline))

/* ==== global tables & per-thread allocation ============================== */

static Pair *g_node = NULL;
static Port *g_vars = NULL;
static uint64_t g_cap_nodes = 0, g_cap_vars = 0;
static uint64_t g_node_next = 1, g_var_next = 1; /* atomic; index 0 is reserved */
static int g_net_ready = 0;

#define CHUNK 4096

typedef struct { uint32_t *v; size_t n, cap; } u32vec;

/* Per-thread allocator, fetched once per worker and passed explicitly
 * (thread-local lookups are function calls on some platforms). */
typedef struct {
    u32vec node_free, var_free;
    uint64_t nb, ne, vb, ve; /* private bump ranges */
    int64_t nodes;           /* live-node delta, flushed into the leak counter */
} NetAlloc;

static _Thread_local NetAlloc t_alloc;

HOT void vec_push(u32vec *v, uint32_t x) {
    if (v->n == v->cap) {
        v->cap = v->cap ? v->cap * 2 : 1024;
        v->v = (uint32_t *)realloc(v->v, v->cap * sizeof(uint32_t));
        if (!v->v) mim_panic("out of memory");
    }
    v->v[v->n++] = x;
}

static void net_init(void) {
    if (g_net_ready) return;
    const char *e = getenv("MIMIC_NET_NODES");
    uint64_t n = (e && atoll(e) > 0) ? (uint64_t)atoll(e) : ((uint64_t)1 << 24);
    if (n > 0xFFFFFFF0u) n = 0xFFFFFFF0u;
    /* large callocs are lazily committed by the OS on Linux/macOS */
    g_node = (Pair *)calloc(n, sizeof(Pair));
    g_vars = (Port *)calloc(n, sizeof(Port));
    if (!g_node || !g_vars) mim_panic("cannot allocate the interaction net (lower MIMIC_NET_NODES)");
    g_cap_nodes = g_cap_vars = n;
    g_net_ready = 1;
}

static void alloc_flush(NetAlloc *al) {
    if (al->nodes) {
        live_slot()->nodes += al->nodes;
        al->nodes = 0;
    }
}

HOT uint32_t node_alloc(NetAlloc *al) {
    al->nodes++;
    if (al->node_free.n) return al->node_free.v[--al->node_free.n];
    if (al->nb == al->ne) {
        uint64_t s = __atomic_fetch_add(&g_node_next, CHUNK, __ATOMIC_RELAXED);
        if (s + CHUNK > g_cap_nodes) mim_panic("interaction net out of nodes (raise MIMIC_NET_NODES)");
        al->nb = s;
        al->ne = s + CHUNK;
    }
    return (uint32_t)al->nb++;
}

HOT void node_free(NetAlloc *al, uint32_t loc) {
    al->nodes--;
    vec_push(&al->node_free, loc);
}

HOT uint32_t var_alloc(NetAlloc *al) {
    uint32_t x;
    if (al->var_free.n) {
        x = al->var_free.v[--al->var_free.n];
    } else {
        if (al->vb == al->ve) {
            uint64_t s = __atomic_fetch_add(&g_var_next, CHUNK, __ATOMIC_RELAXED);
            if (s + CHUNK > g_cap_vars) mim_panic("interaction net out of wires (raise MIMIC_NET_NODES)");
            al->vb = s;
            al->ve = s + CHUNK;
        }
        x = (uint32_t)al->vb++;
    }
    g_vars[x] = NONE; /* unpublished until a port referencing it is linked */
    return x;
}

HOT void var_free(NetAlloc *al, uint32_t x) {
    g_vars[x] = 0; /* only the owner of both ends frees a wire */
    vec_push(&al->var_free, x);
}

HOT Pair node_take(NetAlloc *al, uint32_t loc) {
    Pair p = g_node[loc];
    node_free(al, loc);
    return p;
}

/* ==== numbers ============================================================ */

#define INLINE_MIN (-((int64_t)1 << 57))
#define INLINE_MAX (((int64_t)1 << 57) - 1)

static Port num_box(NetAlloc *al, int dtype, uint64_t bits) {
    uint32_t loc = node_alloc(al);
    g_node[loc].a = (Port)dtype;
    g_node[loc].b = bits;
    return ((uint64_t)loc << 6) | ((uint64_t)NK_BOX << 4) | T_NUM;
}

HOT Port num_int(NetAlloc *al, int64_t x) {
    if (x >= INLINE_MIN && x <= INLINE_MAX) return ((uint64_t)x << 6) | ((uint64_t)NK_INT << 4) | T_NUM;
    return num_box(al, MIM_I64, (uint64_t)x);
}

static Port num_f64(NetAlloc *al, double x) {
    uint64_t bits;
    memcpy(&bits, &x, 8);
    return num_box(al, MIM_F64, bits);
}

static Port num_bool(int64_t b) { return ((uint64_t)(b != 0) << 6) | ((uint64_t)NK_BOOL << 4) | T_NUM; }
static Port num_char(int64_t c) { return ((uint64_t)(c & 0xFF) << 6) | ((uint64_t)NK_CHAR << 4) | T_NUM; }

typedef struct { int dtype; int64_t i; double f; } numv;

/* Read a NUM port without consuming it. */
HOT numv num_peek(Port p) {
    numv v;
    switch ((uint32_t)((p >> 4) & 3)) {
    case NK_INT: v.dtype = MIM_I64; v.i = (int64_t)p >> 6; v.f = (double)v.i; break;
    case NK_BOOL: v.dtype = MIM_BOOL; v.i = (int64_t)(p >> 6); v.f = (double)v.i; break;
    case NK_CHAR: v.dtype = MIM_CHAR; v.i = (int64_t)(p >> 6); v.f = (double)v.i; break;
    default: {
        Pair *b = &g_node[(uint32_t)(p >> 6)];
        v.dtype = (int)b->a;
        if (v.dtype == MIM_F64) { memcpy(&v.f, &b->b, 8); v.i = (int64_t)v.f; }
        else { v.i = (int64_t)b->b; v.f = (double)v.i; }
    }
    }
    return v;
}

HOT int num_is_boxed(Port p) { return P_TAG(p) == T_NUM && ((p >> 4) & 3) == NK_BOX; }

/* Consume a value port: free its box / release its array. */
HOT void value_drop(NetAlloc *al, Port p) {
    if (num_is_boxed(p)) node_free(al, (uint32_t)(p >> 6));
    else if (P_TAG(p) == T_ARR) mim_release((mim_val *)(uintptr_t)P_PAY(p));
}

/* Duplicate a value port: new box / retained array. */
static Port value_copy(NetAlloc *al, Port p) {
    if (num_is_boxed(p)) {
        Pair b = g_node[(uint32_t)(p >> 6)];
        return num_box(al, (int)b.a, b.b);
    }
    if (P_TAG(p) == T_ARR) mim_retain((mim_val *)(uintptr_t)P_PAY(p));
    return p;
}

static Port num_make(NetAlloc *al, numv v) {
    switch (v.dtype) {
    case MIM_F64: return num_f64(al, v.f);
    case MIM_BOOL: return num_bool(v.i);
    case MIM_CHAR: return num_char(v.i);
    default: return num_int(al, v.i);
    }
}

/* borrowed mim_val → owned port */
static Port port_from_val(NetAlloc *al, mim_val *v) {
    if (!v) mim_panic("interaction net: unit value `()` is not a number");
    if (v->rank == 0 && !IS_HANDLES(v)) { /* numbers travel unboxed; records stay handles */
        numv n = {v->dtype, geti(v, 0), getf(v, 0)};
        return num_make(al, n);
    }
    mim_retain(v);
    return MK(T_ARR, (uintptr_t)v);
}

/* owned port → owned mim_val */
static mim_val *val_from_port(NetAlloc *al, Port p) {
    if (P_TAG(p) == T_ARR) return (mim_val *)(uintptr_t)P_PAY(p);
    if (P_TAG(p) != T_NUM) mim_panic("interaction net: result is not a value");
    numv n = num_peek(p);
    value_drop(al, p);
    switch (n.dtype) {
    case MIM_F64: return mim_float(n.f);
    case MIM_BOOL: return mim_bool(n.i);
    case MIM_CHAR: {
        mim_val *c = alloc(MIM_CHAR, 0, 1);
        ((uint8_t *)c->data)[0] = (uint8_t)n.i;
        return c;
    }
    default: return mim_int(n.i);
    }
}

/* owned mim_val → owned port (scalars are unboxed back into the net) */
static Port port_from_owned_val(NetAlloc *al, mim_val *v) {
    if (!v) mim_panic("interaction net: unit value `()` is not a number");
    if (v->rank == 0 && !IS_HANDLES(v)) {
        Port p = port_from_val(al, v);
        mim_release(v);
        return p;
    }
    return MK(T_ARR, (uintptr_t)v);
}

/* ==== scalar arithmetic (same semantics as the array kernels) ============ */

static Port scalar_binop(NetAlloc *al, int op, numv a, numv b) {
    if (IS_BITOP(op)) {
        if (a.dtype == MIM_F64 || b.dtype == MIM_F64 || a.dtype == MIM_CHAR || b.dtype == MIM_CHAR)
            mim_panic("bitwise operators need integers");
        int64_t x = a.i, y = b.i;
        int both_bool = a.dtype == MIM_BOOL && b.dtype == MIM_BOOL;
        switch (op) {
        case OP_BAND: return both_bool ? num_bool(x & y) : num_int(al, x & y);
        case OP_BOR: return both_bool ? num_bool(x | y) : num_int(al, x | y);
        case OP_BXOR: return both_bool ? num_bool(x ^ y) : num_int(al, x ^ y);
        case OP_SHL: return num_int(al, SHL64(x, y));
        case OP_SHR: return num_int(al, SHR64(x, y));
        case OP_USHR: return num_int(al, USHR64(x, y));
        }
    }
    if (a.dtype == MIM_F64 || b.dtype == MIM_F64 || op == OP_DIV) {
        double x = a.f, y = b.f;
        switch (op) {
        case OP_ADD: return num_f64(al, x + y);
        case OP_SUB: return num_f64(al, x - y);
        case OP_MUL: return num_f64(al, x * y);
        case OP_DIV: return num_f64(al, x / y);
        case OP_MOD: return num_f64(al, fmod(x, y));
        case OP_EQ: return num_bool(x == y);
        case OP_NE: return num_bool(x != y);
        case OP_LT: return num_bool(x < y);
        case OP_LE: return num_bool(x <= y);
        case OP_GT: return num_bool(x > y);
        case OP_GE: return num_bool(x >= y);
        case OP_AND: return num_bool(x != 0 && y != 0);
        case OP_OR: return num_bool(x != 0 || y != 0);
        case OP_IDIV: return num_f64(al, floor(x / y));
        case OP_MAX2: return num_f64(al, x > y ? x : y);
        case OP_MIN2: return num_f64(al, x < y ? x : y);
        }
    } else {
        int64_t x = a.i, y = b.i;
        switch (op) {
        case OP_ADD: return num_int(al, (int64_t)((uint64_t)x + (uint64_t)y));
        case OP_SUB: return num_int(al, (int64_t)((uint64_t)x - (uint64_t)y));
        case OP_MUL: return num_int(al, (int64_t)((uint64_t)x * (uint64_t)y));
        case OP_MOD:
            if (y == 0) mim_panic("integer modulo by zero");
            return num_int(al, floormod(x, y));
        case OP_EQ: return num_bool(x == y);
        case OP_NE: return num_bool(x != y);
        case OP_LT: return num_bool(x < y);
        case OP_LE: return num_bool(x <= y);
        case OP_GT: return num_bool(x > y);
        case OP_GE: return num_bool(x >= y);
        case OP_AND: return num_bool(x && y);
        case OP_OR: return num_bool(x || y);
        case OP_IDIV: return num_int(al, floordiv(x, y));
        case OP_MAX2: return num_int(al, x > y ? x : y);
        case OP_MIN2: return num_int(al, x < y ? x : y);
        }
    }
    mim_panic("interaction net: unknown operator");
    return ERA_P;
}

/* Binary op on two consumed value ports; arrays go to the parallel kernels. */
static Port apply2(NetAlloc *al, int op, Port a, Port b) {
    if (P_TAG(a) == T_NUM && P_TAG(b) == T_NUM && op < OP_PRIM1) {
        Port r = scalar_binop(al, op, num_peek(a), num_peek(b));
        value_drop(al, a);
        value_drop(al, b);
        return r;
    }
    mim_val *x = val_from_port(al, a), *y = val_from_port(al, b);
    mim_val *r = op == OP_INDEX ? mim_index(x, y) : op >= OP_PRIM2 ? mim_prim2(op - OP_PRIM2, x, y) : mim_binop(op, x, y);
    mim_release(x);
    mim_release(y);
    return port_from_owned_val(al, r);
}

static Port apply1(NetAlloc *al, int op, Port a) {
    if (P_TAG(a) == T_NUM && (op == OP_NEG || op == OP_NOT || op == OP_ABS || op == OP_BNOT)) {
        numv v = num_peek(a);
        value_drop(al, a);
        if (op == OP_BNOT) {
            if (v.dtype == MIM_F64 || v.dtype == MIM_CHAR) mim_panic("`~` needs an integer");
            return v.dtype == MIM_BOOL ? num_bool(!v.i) : num_int(al, ~v.i);
        }
        if (op == OP_NOT) return num_bool(v.f == 0);
        if (op == OP_ABS) return v.dtype == MIM_F64 ? num_f64(al, fabs(v.f)) : num_int(al, v.i < 0 ? -v.i : v.i);
        return v.dtype == MIM_F64 ? num_f64(al, -v.f) : num_int(al, -v.i);
    }
    mim_val *x = val_from_port(al, a);
    mim_val *r = op >= OP_PRIM1 ? mim_prim1(op - OP_PRIM1, x) : mim_unop(op - OP_NEG, x);
    mim_release(x);
    return port_from_owned_val(al, r);
}

static const char *const *g_check_msgs = NULL;

/* Type-annotation check inside the net: same rules and messages as native code. */
static Port apply_check(NetAlloc *al, uint32_t aux, Port a) {
    int ty = (int)(aux & 0xFF) - OP_CHECK;
    const char *what = g_check_msgs ? g_check_msgs[aux >> 12] : "value";
    if (P_TAG(a) == T_ARR) {
        mim_check_type((mim_val *)(uintptr_t)P_PAY(a), ty, what);
        return a;
    }
    numv v = num_peek(a);
    int ok = (ty == 1 && (v.dtype == MIM_I64 || v.dtype == MIM_BOOL)) || (ty == 2 && v.dtype != MIM_CHAR) ||
             (ty == 3 && v.dtype == MIM_BOOL) || (ty == 4 && v.dtype == MIM_CHAR);
    if (!ok) {
        mim_val *x = val_from_port(al, a);
        mim_check_type(x, ty, what); /* panics with the standard message */
    }
    return a;
}

static int op_is_unary(int op) {
    return op == OP_NEG || op == OP_NOT || op == OP_ABS || op == OP_BNOT || (op >= OP_PRIM1 && op < OP_PRIM2) || (op >= OP_CHECK && op < OP_CHECK + 16);
}

/* ==== the book (global definitions) ====================================== */

typedef struct {
    uint32_t nnodes, nvars, nredex, nlits;
    Port root;
    const Port *nodes;    /* 2 per node */
    const Port *redex;    /* 2 per redex */
    const uint64_t *lits; /* (dtype, bits) per literal */
} Def;

static Def *g_defs = NULL;
static uint32_t g_ndefs = 0;

/* Book layout per def at words[offsets[i]]:
 *   nnodes nvars nredex nlits root | nodes… | redexes… | lits… */
void mim_net_book(const uint64_t *words, const uint64_t *offsets, uint32_t ndefs) {
    net_init();
    g_defs = (Def *)realloc(g_defs, sizeof(Def) * (ndefs ? ndefs : 1));
    for (uint32_t i = 0; i < ndefs; i++) {
        const uint64_t *w = words + offsets[i];
        Def *d = &g_defs[i];
        d->nnodes = (uint32_t)w[0];
        d->nvars = (uint32_t)w[1];
        d->nredex = (uint32_t)w[2];
        d->nlits = (uint32_t)w[3];
        d->root = w[4];
        d->nodes = w + 5;
        d->redex = d->nodes + 2 * d->nnodes;
        d->lits = d->redex + 2 * d->nredex;
    }
    g_ndefs = ndefs;
}

/* ==== native fallback ====================================================
 *
 * Compiled programs register, per definition, the native (LLVM) version of
 * the function it came from. When a call is about to be expanded in the net,
 * all of its arguments are already values, and every worker is busy, the
 * runtime calls the native version instead: the net is used to *split* work
 * while cores are idle, and each core then runs its share at native speed.
 * Native recursion is bounded (NATIVE_LIMIT frames); deeper calls go back to
 * the heap-allocated net, so deep recursion cannot overflow the stack. */

typedef mim_val *(*nat0)(void);
typedef mim_val *(*nat1)(mim_val *);
typedef mim_val *(*nat2)(mim_val *, mim_val *);
typedef mim_val *(*nat3)(mim_val *, mim_val *, mim_val *);
typedef mim_val *(*nat4)(mim_val *, mim_val *, mim_val *, mim_val *);
typedef mim_val *(*nat5)(mim_val *, mim_val *, mim_val *, mim_val *, mim_val *);
typedef mim_val *(*nat6)(mim_val *, mim_val *, mim_val *, mim_val *, mim_val *, mim_val *);

static void *const *g_native = NULL;
static const uint8_t *g_native_arity = NULL;

/* WebAssembly engines give wasm one ~1 MB native stack (V8): hand deep
 * recursion to the heap-based reducer much sooner there. Must match codegen. */
#ifdef __wasm__
#define NATIVE_LIMIT 256
#else
#define NATIVE_LIMIT 2048
#endif
static _Thread_local int32_t t_native_depth = 0;

/* Compiled native code tracks its recursion depth in a hidden argument and
 * only calls these when it hands a deep call to the net: while inside, the
 * runtime must not fall back to native code again on this thread. */
void mim_deep_enter(void) { t_native_depth += NATIVE_LIMIT; }
void mim_deep_leave(void) { t_native_depth -= NATIVE_LIMIT; }

/* Thread-safe one-time registration, used by compiled programs: the first
 * caller loads the book, concurrent callers wait for it. */
void mim_net_book_once(const uint64_t *words, const uint64_t *offsets, uint32_t ndefs, void *const *natives,
                       const uint8_t *arity, const char *const *check_msgs) {
    static int state = 0; /* 0 = empty, 1 = loading, 2 = ready */
    if (__atomic_load_n(&state, __ATOMIC_ACQUIRE) == 2) return;
    int expected = 0;
    if (__atomic_compare_exchange_n(&state, &expected, 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        mim_net_book(words, offsets, ndefs);
        g_native = natives;
        g_native_arity = arity;
        g_check_msgs = check_msgs;
        __atomic_store_n(&state, 2, __ATOMIC_RELEASE);
        return;
    }
    while (__atomic_load_n(&state, __ATOMIC_ACQUIRE) != 2) {
#if MIM_THREADS
        sched_yield();
#elif MIM_THREADS
        SwitchToThread();
#endif
    }
}

/* ==== workers, bags and runs =============================================
 *
 * Each worker reduces from a private stack with no synchronisation. Only
 * while another worker is idle does it move the *oldest* half of that stack
 * (the biggest pending subproblems) into its shared bag, where idle workers
 * steal from. */

typedef struct { Port a, b; } Redex;

typedef struct {
    mim_mutex mu;
    Redex *buf;
    uint64_t cap;
    uint64_t head, tail; /* items in [head, tail); read atomically outside the lock */
    char pad[64];
} Bag;

typedef struct {
    int nworkers;
    Bag *bags;
    int64_t participants, idle; /* atomic */
    int done;                   /* atomic */
    uint64_t interactions;      /* atomic */
} Run;

typedef struct {
    Run *run;
    int id;
    int par; /* other workers may touch shared wires: use atomics */
    Bag *bag;
    NetAlloc *al;
    uint64_t itrs;
    Redex *loc;
    uint64_t lh, lt, lcap; /* private redexes in [lh, lt) */
} TM;

static void tm_init(TM *tm, Run *r, int id) {
    memset(tm, 0, sizeof *tm);
    tm->run = r;
    tm->id = id;
    tm->bag = &r->bags[id];
    tm->al = &t_alloc;
    tm->par = r->nworkers > 1;
}

static void tm_done(TM *tm) {
    free(tm->loc);
    tm->loc = NULL;
    alloc_flush(tm->al);
}

HOT void push_redex(TM *tm, Port a, Port b) {
    if (tm->lt == tm->lcap) {
        if (tm->lh > 0) {
            memmove(tm->loc, tm->loc + tm->lh, (size_t)(tm->lt - tm->lh) * sizeof(Redex));
            tm->lt -= tm->lh;
            tm->lh = 0;
        }
        if (tm->lt == tm->lcap) {
            tm->lcap = tm->lcap ? tm->lcap * 2 : 4096;
            tm->loc = (Redex *)realloc(tm->loc, (size_t)tm->lcap * sizeof(Redex));
            if (!tm->loc) mim_panic("out of memory");
        }
    }
    tm->loc[tm->lt++] = (Redex){a, b};
}

/* caller holds g->mu */
static void bag_append_locked(Bag *g, Redex x) {
    uint64_t n = g->tail - g->head;
    if (n == g->cap) {
        uint64_t nc = g->cap ? g->cap * 2 : 1024;
        Redex *nb = (Redex *)malloc(sizeof(Redex) * nc);
        if (!nb) mim_panic("out of memory");
        for (uint64_t i = 0; i < n; i++) nb[i] = g->buf[(g->head + i) % g->cap];
        free(g->buf);
        g->buf = nb;
        g->cap = nc;
        __atomic_store_n(&g->head, 0, __ATOMIC_RELAXED);
        __atomic_store_n(&g->tail, n, __ATOMIC_RELAXED);
    }
    g->buf[g->tail % g->cap] = x;
    __atomic_store_n(&g->tail, g->tail + 1, __ATOMIC_RELEASE);
}

static int bag_nonempty(Bag *g) {
    return __atomic_load_n(&g->tail, __ATOMIC_ACQUIRE) != __atomic_load_n(&g->head, __ATOMIC_ACQUIRE);
}

/* Owner: newest first. */
static int bag_pop(Bag *g, Redex *out) {
    if (!bag_nonempty(g)) return 0;
    int ok = 0;
    mutex_lock(&g->mu);
    if (g->tail > g->head) {
        *out = g->buf[(g->tail - 1) % g->cap];
        __atomic_store_n(&g->tail, g->tail - 1, __ATOMIC_RELEASE);
        ok = 1;
    }
    mutex_unlock(&g->mu);
    return ok;
}

/* Thief: oldest first. */
static int bag_steal(Bag *g, Redex *out) {
    if (!bag_nonempty(g)) return 0;
    if (!mutex_trylock(&g->mu)) return 0;
    int ok = 0;
    if (g->tail > g->head) {
        *out = g->buf[g->head % g->cap];
        __atomic_store_n(&g->head, g->head + 1, __ATOMIC_RELEASE);
        ok = 1;
    }
    mutex_unlock(&g->mu);
    return ok;
}

/* Move the older half of the private stack into the shared bag. */
static void share(TM *tm) {
    uint64_t k = (tm->lt - tm->lh) / 2;
    mutex_lock(&tm->bag->mu);
    for (uint64_t i = 0; i < k; i++) bag_append_locked(tm->bag, tm->loc[tm->lh + i]);
    mutex_unlock(&tm->bag->mu);
    tm->lh += k;
}

/* Move everything private into the shared bag (hand-off before workers start). */
static void publish_all(TM *tm) {
    mutex_lock(&tm->bag->mu);
    for (uint64_t i = tm->lh; i < tm->lt; i++) bag_append_locked(tm->bag, tm->loc[i]);
    mutex_unlock(&tm->bag->mu);
    tm->lh = tm->lt = 0;
}

/* ==== linking ============================================================ */

/* Exchange a wire slot: atomic when other workers run, plain otherwise. */
HOT Port var_xchg(int par, uint64_t x, Port v) {
    if (par) return __atomic_exchange_n(&g_vars[x], v, __ATOMIC_ACQ_REL);
    Port old = g_vars[x];
    g_vars[x] = v;
    return old;
}

/* Follow a var whose other end has already been resolved. */
HOT Port enter(NetAlloc *al, int par, Port var) {
    while (P_TAG(var) == T_VAR) {
        Port val = var_xchg(par, P_PAY(var), NONE);
        if (val == NONE || val == 0) break;
        var_free(al, (uint32_t)P_PAY(var));
        var = val;
    }
    return var;
}

/* Connect A and B. Two principal ports form a redex; a var is resolved by
 * racing on its slot — whoever arrives second completes the connection. */
HOT void net_link(TM *tm, Port A, Port B) {
    for (;;) {
        if (P_TAG(A) != T_VAR && P_TAG(B) == T_VAR) {
            Port X = A;
            A = B;
            B = X;
        }
        if (P_TAG(A) != T_VAR) {
            push_redex(tm, A, B);
            return;
        }
        B = enter(tm->al, tm->par, B);
        Port A2 = var_xchg(tm->par, P_PAY(A), B);
        if (A2 == NONE) return; /* first to arrive: the other end finishes the job */
        var_free(tm->al, (uint32_t)P_PAY(A));
        A = A2;
    }
}

/* ==== expansion of global definitions (CALL) ============================= */

static Port lit_port(NetAlloc *al, const Def *d, uint64_t i) {
    numv v;
    v.dtype = (int)d->lits[2 * i];
    uint64_t bits = d->lits[2 * i + 1];
    if (v.dtype == MIM_F64) { memcpy(&v.f, &bits, 8); v.i = 0; }
    else { v.i = (int64_t)bits; v.f = (double)v.i; }
    return num_make(al, v);
}

HOT Port subst(NetAlloc *al, const Def *d, Port t, const uint32_t *nloc, const uint32_t *vloc) {
    switch (P_TAG(t)) {
    case T_VAR: return MK(T_VAR, vloc[P_PAY(t)]);
    case T_CON:
    case T_DUP:
    case T_OPR:
    case T_SWI: return MKN(P_TAG(t), nloc[P_LOC(t)], P_AUX(t));
    case T_LIT: return lit_port(al, d, P_PAY(t));
    default: return t;
    }
}

/* Expand definition `id` against `target`.
 *
 * Spine fusion: a call is `@f ~ (a1 (a2 … r))` and f's root is usually
 * `(p1 (p2 … body))`. Instead of building f's root CONs only to annihilate
 * them with the caller's, connect p_i ~ a_i directly. */
static void expand(TM *tm, uint32_t id, Port target) {
    if (id >= g_ndefs) mim_panic("interaction net: unknown definition");
    const Def *d = &g_defs[id];
    NetAlloc *al = tm->al;
    uint32_t sbuf[512];
    uint32_t *nloc = sbuf, *vloc = sbuf + 256, *heap = NULL;
    if (d->nnodes > 256 || d->nvars > 256) {
        heap = (uint32_t *)malloc(sizeof(uint32_t) * ((size_t)d->nnodes + d->nvars + 2));
        if (!heap) mim_panic("out of memory");
        nloc = heap;
        vloc = heap + d->nnodes + 1;
    }

    /* walk the matching CON spines */
#define MAX_SPINE 32
    Port spine_tpl[MAX_SPINE], spine_arg[MAX_SPINE];
    int nspine = 0;
    Port tp = d->root, tg = target;
    while (nspine < MAX_SPINE && P_TAG(tp) == T_CON && P_TAG(tg) == T_CON && P_AUX(tp) == 0 && P_AUX(tg) == 0) {
        uint32_t tl = P_LOC(tp);
        Pair arg = node_take(al, P_LOC(tg));
        spine_tpl[nspine] = d->nodes[2 * tl];
        spine_arg[nspine] = arg.a;
        nspine++;
        tp = d->nodes[2 * tl + 1];
        tg = arg.b;
    }
    if (nspine) tm->itrs += (uint64_t)nspine; /* count the annihilations we skipped */

    /* mark spine nodes so they are never materialised */
    if (nspine)
        for (uint32_t i = 0; i < d->nnodes; i++) nloc[i] = 0;
    Port walk = d->root;
    for (int i = 0; i < nspine; i++) {
        nloc[P_LOC(walk)] = UINT32_MAX;
        walk = d->nodes[2 * P_LOC(walk) + 1];
    }
    for (uint32_t i = 0; i < d->nnodes; i++) {
        if (nspine && nloc[i] == UINT32_MAX) continue;
        nloc[i] = node_alloc(al);
    }
    for (uint32_t i = 0; i < d->nvars; i++) vloc[i] = var_alloc(al);
    for (uint32_t i = 0; i < d->nnodes; i++) {
        if (nspine && nloc[i] == UINT32_MAX) continue;
        g_node[nloc[i]].a = subst(al, d, d->nodes[2 * i], nloc, vloc);
        g_node[nloc[i]].b = subst(al, d, d->nodes[2 * i + 1], nloc, vloc);
    }
    for (uint32_t i = 0; i < d->nredex; i++)
        net_link(tm, subst(al, d, d->redex[2 * i], nloc, vloc), subst(al, d, d->redex[2 * i + 1], nloc, vloc));
    for (int i = 0; i < nspine; i++) net_link(tm, subst(al, d, spine_tpl[i], nloc, vloc), spine_arg[i]);
    net_link(tm, subst(al, d, tp, nloc, vloc), tg);
    if (heap) free(heap);
}

/* If `p` is a value (or a wire whose other end already holds one), return it,
 * taking ownership; otherwise 0. Never blocks and never changes an unready wire. */
HOT Port ready_value(TM *tm, Port p) {
    uint32_t t = P_TAG(p);
    if (t == T_NUM || t == T_ARR) return p;
    if (t != T_VAR) return 0;
    Port v = tm->par ? __atomic_load_n(&g_vars[P_PAY(p)], __ATOMIC_ACQUIRE) : g_vars[P_PAY(p)];
    if (v == NONE || v == 0 || (P_TAG(v) != T_NUM && P_TAG(v) != T_ARR)) return 0;
    var_free(tm->al, (uint32_t)P_PAY(p)); /* the other end is done with it: we own both */
    return v;
}

static void share(TM *tm);
static void publish_all(TM *tm);

static int try_native(TM *tm, uint32_t id, Port target) {
    if (!g_native || !g_native[id] || P_TAG(target) != T_CON) return 0;
    if (t_native_depth >= NATIVE_LIMIT) return 0;
    if (tm->par) {
        /* only when every worker is busy, and others still have work to steal */
        if (__atomic_load_n(&tm->run->idle, __ATOMIC_RELAXED) > 0) return 0;
        if (tm->lt == tm->lh && !bag_nonempty(tm->bag)) return 0;
    }
    int ar = g_native_arity[id];
    uint32_t locs[6];
    Port spine = target;
    for (int i = 0; i < ar; i++) {
        if (P_TAG(spine) != T_CON || P_AUX(spine) != 0) return 0;
        uint32_t loc = P_LOC(spine);
        Port a = ready_value(tm, g_node[loc].a);
        if (!a) return 0; /* not ready: expand in the net (resolved args stay resolved) */
        g_node[loc].a = a;
        locs[i] = loc;
        spine = g_node[loc].b;
    }
    NetAlloc *al = tm->al;
    mim_val *args[6];
    for (int i = 0; i < ar; i++) {
        args[i] = val_from_port(al, g_node[locs[i]].a);
        node_free(al, locs[i]);
    }
    if (tm->par && tm->lt > tm->lh) publish_all(tm); /* let thieves progress meanwhile */
    void *f = g_native[id];
    mim_val *r;
    switch (ar) {
    case 0: r = ((nat0)f)(); break;
    case 1: r = ((nat1)f)(args[0]); break;
    case 2: r = ((nat2)f)(args[0], args[1]); break;
    case 3: r = ((nat3)f)(args[0], args[1], args[2]); break;
    case 4: r = ((nat4)f)(args[0], args[1], args[2], args[3]); break;
    case 5: r = ((nat5)f)(args[0], args[1], args[2], args[3], args[4]); break;
    default: r = ((nat6)f)(args[0], args[1], args[2], args[3], args[4], args[5]); break;
    }
    for (int i = 0; i < ar; i++) mim_release(args[i]);
    net_link(tm, port_from_owned_val(al, r), spine);
    return 1;
}

/* ==== interaction rules ================================================== */

HOT int is_nullary(uint32_t t) { return t == T_ERA || t == T_NUM || t == T_ARR || t == T_REF; }

static void interact(TM *tm, Port a, Port b) {
    NetAlloc *al = tm->al;
    tm->itrs++;
    uint32_t ta = P_TAG(a), tb = P_TAG(b);
    if (!is_nullary(ta) && is_nullary(tb)) {
        Port x = a; a = b; b = x;
        uint32_t t = ta; ta = tb; tb = t;
    }

    if (is_nullary(ta) && is_nullary(tb)) { /* VOID */
        value_drop(al, a);
        value_drop(al, b);
        return;
    }

    if (ta == T_REF) {
        if (tb == T_DUP) { /* copy the reference: recursion stays lazy */
            Pair n = node_take(al, P_LOC(b));
            net_link(tm, n.a, a);
            net_link(tm, n.b, a);
        } else if (!try_native(tm, (uint32_t)P_PAY(a), b)) { /* CALL */
            expand(tm, (uint32_t)P_PAY(a), b);
        }
        return;
    }

    if (ta == T_ERA) { /* ERASE */
        Pair n = node_take(al, P_LOC(b));
        net_link(tm, n.a, ERA_P);
        net_link(tm, n.b, ERA_P);
        return;
    }

    if (is_nullary(ta)) { /* NUM / ARR against a binary node */
        switch (tb) {
        case T_DUP: { /* COPY */
            Pair n = node_take(al, P_LOC(b));
            net_link(tm, n.a, value_copy(al, a));
            net_link(tm, n.b, a);
            return;
        }
        case T_OPR: { /* OPERATE */
            uint32_t aux = P_AUX(b);
            int op = (int)(aux & 0xFF), flip = (aux & OP_FLIP) != 0;
            Pair n = node_take(al, P_LOC(b));
            if (op_is_unary(op)) {
                net_link(tm, n.a, ERA_P);
                net_link(tm, n.b, op >= OP_CHECK ? apply_check(al, aux, a) : apply1(al, op, a));
                return;
            }
            uint32_t tx = P_TAG(n.a);
            if (tx == T_NUM || tx == T_ARR) {
                net_link(tm, n.b, flip ? apply2(al, op, n.a, a) : apply2(al, op, a, n.a));
            } else { /* the other operand is not ready: wait for it, holding `a` */
                uint32_t m = node_alloc(al);
                g_node[m].a = a;
                g_node[m].b = n.b;
                net_link(tm, n.a, MKN(T_OPR, m, (uint32_t)op | (flip ? 0 : OP_FLIP)));
            }
            return;
        }
        case T_SWI: { /* SWITCH: keep one branch, erase the other unevaluated */
            int64_t truth;
            if (ta == T_NUM) {
                truth = num_peek(a).f != 0;
                value_drop(al, a);
            } else {
                mim_val *v = val_from_port(al, a);
                truth = mim_truthy(v);
                mim_release(v);
            }
            Pair n = node_take(al, P_LOC(b));
            if (P_TAG(n.a) == T_CON && P_AUX(n.a) == 0) {
                /* the branch pair is right here: pick one, erase the other
                 * (counted as the CON~CON annihilation it replaces) */
                Pair br = node_take(al, P_LOC(n.a));
                tm->itrs++;
                net_link(tm, truth ? br.a : br.b, n.b);
                net_link(tm, truth ? br.b : br.a, ERA_P);
                return;
            }
            uint32_t m = node_alloc(al);
            g_node[m].a = truth ? n.b : ERA_P;
            g_node[m].b = truth ? ERA_P : n.b;
            net_link(tm, n.a, MKN(T_CON, m, 0));
            return;
        }
        default:
            mim_panic("interaction net: a number was used as a function (type error in net)");
        }
    }

    /* two binary nodes */
    if (ta == tb && P_AUX(a) == P_AUX(b) && (ta == T_CON || ta == T_DUP)) { /* ANNIHILATE */
        Pair x = node_take(al, P_LOC(a)), y = node_take(al, P_LOC(b));
        net_link(tm, x.a, y.a);
        net_link(tm, x.b, y.b);
        return;
    }
    { /* COMMUTE */
        Pair x = node_take(al, P_LOC(a)), y = node_take(al, P_LOC(b));
        uint32_t xa = P_AUX(a), ya = P_AUX(b);
        uint32_t n0 = node_alloc(al), n1 = node_alloc(al), n2 = node_alloc(al), n3 = node_alloc(al);
        uint32_t v0 = var_alloc(al), v1 = var_alloc(al), v2 = var_alloc(al), v3 = var_alloc(al);
        g_node[n0] = (Pair){MK(T_VAR, v0), MK(T_VAR, v1)};
        g_node[n1] = (Pair){MK(T_VAR, v2), MK(T_VAR, v3)};
        g_node[n2] = (Pair){MK(T_VAR, v0), MK(T_VAR, v2)};
        g_node[n3] = (Pair){MK(T_VAR, v1), MK(T_VAR, v3)};
        net_link(tm, x.a, MKN(tb, n0, ya));
        net_link(tm, x.b, MKN(tb, n1, ya));
        net_link(tm, y.a, MKN(ta, n2, xa));
        net_link(tm, y.b, MKN(ta, n3, xa));
    }
}

/* ==== the reducer ======================================================== */

static void net_work(TM *tm) {
    Run *r = tm->run;
    __atomic_fetch_add(&r->participants, 1, __ATOMIC_ACQ_REL);
    Redex x;
    for (;;) {
        /* hot path: private stack, no synchronisation */
        while (tm->lt > tm->lh) {
            x = tm->loc[--tm->lt];
            interact(tm, x.a, x.b);
            if (r->nworkers > 1 && (tm->itrs & 31) == 0 && tm->lt - tm->lh > 1 &&
                __atomic_load_n(&r->idle, __ATOMIC_RELAXED) > 0 && !bag_nonempty(tm->bag))
                share(tm);
        }
        tm->lh = tm->lt = 0;
        if (bag_pop(tm->bag, &x)) {
            interact(tm, x.a, x.b);
            continue;
        }
        int stolen = 0;
        for (int k = 1; k < r->nworkers && !stolen; k++)
            stolen = bag_steal(&r->bags[(tm->id + k) % r->nworkers], &x);
        if (stolen) {
            interact(tm, x.a, x.b);
            continue;
        }
        if (r->nworkers == 1) break;
        /* Idle. The run is finished when every participant is idle: only a
         * busy owner adds work to its bag, so all bags are then empty. */
        __atomic_fetch_add(&r->idle, 1, __ATOMIC_ACQ_REL);
        for (;;) {
            if (__atomic_load_n(&r->done, __ATOMIC_ACQUIRE)) goto out;
            if (__atomic_load_n(&r->idle, __ATOMIC_ACQUIRE) == __atomic_load_n(&r->participants, __ATOMIC_ACQUIRE)) {
                __atomic_store_n(&r->done, 1, __ATOMIC_RELEASE);
                goto out;
            }
            int any = 0;
            for (int k = 0; k < r->nworkers && !any; k++) any = bag_nonempty(&r->bags[k]);
            if (any) {
                __atomic_fetch_sub(&r->idle, 1, __ATOMIC_ACQ_REL);
                break;
            }
#if MIM_THREADS
            sched_yield();
#elif MIM_THREADS
            SwitchToThread();
#endif
        }
    }
out:
    __atomic_fetch_add(&r->interactions, tm->itrs, __ATOMIC_RELAXED);
}

static void net_chunk(void *ctx, int64_t lo, int64_t hi) {
    Run *r = (Run *)ctx;
    for (int64_t id = lo; id < hi; id++) {
        if (__atomic_load_n(&r->done, __ATOMIC_ACQUIRE)) return;
        TM tm;
        tm_init(&tm, r, (int)id);
        net_work(&tm);
        tm_done(&tm);
    }
}

static uint64_t g_net_interactions = 0; /* atomic, all runs */

/* Reduce everything `seed` created until `root_var` holds a value. */
static mim_val *net_reduce(TM *seed, uint32_t root_var) {
    Run *r = seed->run;
    NetAlloc *al = seed->al;
    for (;;) {
        publish_all(seed); /* whichever thread runs worker 0 picks these up */
        if (r->nworkers > 1) parallel_for_grain(r->nworkers, 1, net_chunk, r);
        else net_chunk(r, 0, 1);
        __atomic_fetch_add(&g_net_interactions, r->interactions, __ATOMIC_RELAXED);
        Port res = enter(al, 0, MK(T_VAR, root_var)); /* workers have joined */
        /* enter() already freed the root wire when it followed it */
        if (P_TAG(res) == T_VAR) mim_panic("interaction net: the result is disconnected");
        if (P_TAG(res) != T_REF) return val_from_port(al, res);
        /* the result is a lazy reference: expand it and keep reducing */
        root_var = var_alloc(al);
        r->participants = r->idle = 0;
        r->done = 0;
        r->interactions = 0;
        expand(seed, (uint32_t)P_PAY(res), MK(T_VAR, root_var));
    }
}

/* Call definition `def` with `nargs` borrowed arguments. The definition's
 * interface is (arg1 (arg2 … result)). Returns an owned value. */
mim_val *mim_net_call(uint32_t def, int32_t nargs, mim_val **args) {
    net_init();
    Run r;
    memset(&r, 0, sizeof r);
    r.nworkers = (g_nthreads > 1 && !t_in_parallel) ? g_nthreads : 1;
    r.bags = (Bag *)calloc((size_t)r.nworkers, sizeof(Bag));
    if (!r.bags) mim_panic("out of memory");
    for (int i = 0; i < r.nworkers; i++) mutex_init(&r.bags[i].mu);

    TM seed;
    tm_init(&seed, &r, 0);
    seed.par = 0; /* setup happens before any worker starts */
    NetAlloc *al = seed.al;
    uint32_t root = var_alloc(al);
    Port target = MK(T_VAR, root);
    for (int32_t i = nargs - 1; i >= 0; i--) {
        uint32_t m = node_alloc(al);
        g_node[m].a = port_from_val(al, args[i]);
        g_node[m].b = target;
        target = MKN(T_CON, m, 0);
    }
    expand(&seed, def, target);
    mim_val *v = net_reduce(&seed, root);
    tm_done(&seed);
    for (int i = 0; i < r.nworkers; i++) free(r.bags[i].buf);
    free(r.bags);
    return v;
}

/* Diagnostics for tests and `mimic net run --stats`. */
uint64_t mim_net_interactions(void) { return __atomic_load_n(&g_net_interactions, __ATOMIC_RELAXED); }

int64_t mim_net_live_nodes(void) {
    int64_t n = 0;
    for (int i = 0; i < MAX_SLOTS; i++) n += __atomic_load_n(&g_live_slots[i].nodes, __ATOMIC_RELAXED);
    return n;
}
