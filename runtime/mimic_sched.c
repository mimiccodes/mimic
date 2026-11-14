/* Mimic runtime — the event loop: green threads (coroutines).
 *
 * Included into mimic_rt.c (after mimic_std.c, before mimic_io.c).
 *
 * Tasks that mostly wait (one per network connection, and the implicit
 * futures they start) run as coroutines instead of operating-system threads.
 * Each coroutine has its own small stack and runs on one of a few scheduler
 * threads, one per core. A coroutine that would block on a socket, a timer, a
 * channel or another task "parks": it switches back to its scheduler, which
 * runs other coroutines meanwhile. One poller thread watches every waiting
 * socket (kqueue on Apple systems, epoll on Linux, IOCP through wepoll on
 * Windows) and every timer, and puts coroutines back on their run queue when
 * they can go on. On Windows the coroutines are fibers.
 *
 * A coroutine always runs on the same scheduler thread (it is never moved),
 * so thread-local state and locks behave as in ordinary threads.
 *
 * Where there is no event loop (WASI, other CPUs, or the environment
 * variable MIMIC_NO_EVENT_LOOP=1), the same functions fall back to threads and
 * blocking calls. */

#if MIM_THREADS && (defined(__APPLE__) || defined(__linux__) || defined(_WIN32)) && (defined(__aarch64__) || defined(__x86_64__))
#define MIM_GREEN 1
#else
#define MIM_GREEN 0
#endif

#if MIM_GREEN
#include <time.h>
#ifdef _WIN32
/* wepoll: epoll on Windows (IOCP and the AFD driver's poll), compiled in.
 * It sets its own Windows version macros: the runtime's are kept. */
#pragma push_macro("_WIN32_WINNT")
#pragma push_macro("WIN32_LEAN_AND_MEAN")
#include "wepoll.c"
#pragma pop_macro("WIN32_LEAN_AND_MEAN")
#pragma pop_macro("_WIN32_WINNT")
/* wepoll declares UNICODE_STRING (same layout as the SDK's): the security
 * headers the TLS code includes later must not declare it again */
#define __UNICODE_STRING_DEFINED
#else
#include <sys/mman.h>
#include <sys/socket.h>
#include <fcntl.h>
#ifdef __APPLE__
#include <sys/event.h>
#else
#include <sys/epoll.h>
#endif
#endif

/* ---- context switching -----------------------------------------------------
 * mim_co_switch(save, to) saves the callee-saved registers on the current
 * stack, stores the stack pointer in *save and continues on the stack `to`.
 * A new coroutine's stack starts with a frame that "returns" into
 * mim_co_boot, which calls mim_co_entry(co). */

#ifdef __APPLE__
#define MIM_SYM(x) "_" #x
#else
#define MIM_SYM(x) #x
#endif

typedef struct mim_co mim_co;
void mim_co_entry(mim_co *co);

#ifdef _WIN32
/* Windows: each coroutine is a fiber (the system switches stacks, and keeps
 * the stack limits that exception handling and stack probes rely on) */
static void mim_co_switch(void **save, void *to) {
    (void)save; /* a fiber's handle never changes */
    SwitchToFiber(to);
}

static void WINAPI co_fiber_main(void *co) { mim_co_entry((mim_co *)co); }
#else
void mim_co_switch(void **save, void *to);
void mim_co_boot(void);

#if defined(__aarch64__)
__asm__(".text\n"
        ".globl " MIM_SYM(mim_co_switch) "\n"
        ".p2align 2\n" MIM_SYM(mim_co_switch) ":\n"
        "  sub sp, sp, #160\n"
        "  stp x19, x20, [sp, #0]\n"
        "  stp x21, x22, [sp, #16]\n"
        "  stp x23, x24, [sp, #32]\n"
        "  stp x25, x26, [sp, #48]\n"
        "  stp x27, x28, [sp, #64]\n"
        "  stp x29, x30, [sp, #80]\n"
        "  stp d8, d9, [sp, #96]\n"
        "  stp d10, d11, [sp, #112]\n"
        "  stp d12, d13, [sp, #128]\n"
        "  stp d14, d15, [sp, #144]\n"
        "  mov x9, sp\n"
        "  str x9, [x0]\n"
        "  mov sp, x1\n"
        "  ldp x19, x20, [sp, #0]\n"
        "  ldp x21, x22, [sp, #16]\n"
        "  ldp x23, x24, [sp, #32]\n"
        "  ldp x25, x26, [sp, #48]\n"
        "  ldp x27, x28, [sp, #64]\n"
        "  ldp x29, x30, [sp, #80]\n"
        "  ldp d8, d9, [sp, #96]\n"
        "  ldp d10, d11, [sp, #112]\n"
        "  ldp d12, d13, [sp, #128]\n"
        "  ldp d14, d15, [sp, #144]\n"
        "  add sp, sp, #160\n"
        "  ret\n"
        ".globl " MIM_SYM(mim_co_boot) "\n"
        ".p2align 2\n" MIM_SYM(mim_co_boot) ":\n"
        "  mov x0, x19\n"
        "  bl " MIM_SYM(mim_co_entry) "\n"
        "  brk #0\n");
#define CO_FRAME 160
static void *co_first_frame(char *top, mim_co *co) {
    uint64_t *f = (uint64_t *)(top - CO_FRAME);
    memset(f, 0, CO_FRAME);
    f[0] = (uint64_t)(uintptr_t)co;                /* x19 */
    f[11] = (uint64_t)(uintptr_t)&mim_co_boot;     /* x30: where `ret` goes */
    return f;
}
#else /* x86_64 */
__asm__(".text\n"
        ".globl " MIM_SYM(mim_co_switch) "\n"
        ".p2align 4\n" MIM_SYM(mim_co_switch) ":\n"
        "  pushq %rbp\n"
        "  pushq %rbx\n"
        "  pushq %r12\n"
        "  pushq %r13\n"
        "  pushq %r14\n"
        "  pushq %r15\n"
        "  subq $8, %rsp\n"
        "  stmxcsr (%rsp)\n"
        "  fnstcw 4(%rsp)\n"
        "  movq %rsp, (%rdi)\n"
        "  movq %rsi, %rsp\n"
        "  ldmxcsr (%rsp)\n"
        "  fldcw 4(%rsp)\n"
        "  addq $8, %rsp\n"
        "  popq %r15\n"
        "  popq %r14\n"
        "  popq %r13\n"
        "  popq %r12\n"
        "  popq %rbx\n"
        "  popq %rbp\n"
        "  ret\n"
        ".globl " MIM_SYM(mim_co_boot) "\n"
        ".p2align 4\n" MIM_SYM(mim_co_boot) ":\n"
        "  movq %rbx, %rdi\n"
        "  call " MIM_SYM(mim_co_entry) "\n"
        "  ud2\n");
static void *co_first_frame(char *top, mim_co *co) {
    /* control words, r15 r14 r13 r12 rbx rbp, return address; after the
     * `ret` the stack is 16-byte aligned, as a call expects */
    uint64_t *f = (uint64_t *)(top - 80);
    memset(f, 0, 80);
    uint32_t mxcsr = 0x1F80;
    uint16_t fcw = 0x037F;
    memcpy((char *)f, &mxcsr, 4);
    memcpy((char *)f + 4, &fcw, 2);
    f[5] = (uint64_t)(uintptr_t)co;            /* rbx */
    f[7] = (uint64_t)(uintptr_t)&mim_co_boot;  /* return address */
    return f;
}
#endif
#endif /* _WIN32 */

/* ---- coroutines and schedulers ------------------------------------------ */

typedef void (*co_park_fn)(mim_co *co, void *arg);

typedef struct co_sched {
    pthread_mutex_t m;
    pthread_cond_t c;
    mim_co *head, *tail; /* the run queue */
    void *sp;            /* the scheduler's own stack while a coroutine runs */
} co_sched;

struct mim_co {
    void *sp;
    char *stack;
    void (*fn)(void *);
    void *arg;
    co_sched *home;
    co_park_fn park; /* set while switching out to wait */
    void *park_arg;
    int32_t native_depth;
    int in_parallel;
    int finished;
    mim_co *next; /* in a run queue or a waiter list */
    /* joining */
    pthread_mutex_t m;
    pthread_cond_t c;
    int done, detached;
    mim_co *joiner;
};

#define CO_STACK ((size_t)32 << 20) /* reserved, not committed: pages appear as used */
#define CO_STACK_CACHE 64

static co_sched *g_sched;
static int g_nsched;
static int g_green = -1; /* -1: not decided yet */
static pthread_mutex_t g_green_lock = PTHREAD_MUTEX_INITIALIZER;
static uint32_t g_next_sched;
static _Thread_local mim_co *t_co; /* the coroutine running on this thread */

#ifndef _WIN32
static char *g_stacks[CO_STACK_CACHE];
static int g_nstacks;
static pthread_mutex_t g_stack_lock = PTHREAD_MUTEX_INITIALIZER;
static size_t g_page;

static char *stack_get(void) {
    pthread_mutex_lock(&g_stack_lock);
    char *s = g_nstacks ? g_stacks[--g_nstacks] : NULL;
    pthread_mutex_unlock(&g_stack_lock);
    if (s) return s;
    int flags = MAP_PRIVATE | MAP_ANON;
#ifdef MAP_NORESERVE
    flags |= MAP_NORESERVE;
#endif
    void *p = mmap(NULL, CO_STACK, PROT_READ | PROT_WRITE, flags, -1, 0);
    if (p == MAP_FAILED) mim_panic("out of memory (coroutine stack)");
    mprotect(p, g_page, PROT_NONE); /* a guard page: overflow faults instead of corrupting */
    return (char *)p;
}

static void stack_put(char *s) {
    pthread_mutex_lock(&g_stack_lock);
    if (g_nstacks < CO_STACK_CACHE) {
        g_stacks[g_nstacks++] = s;
        s = NULL;
    }
    pthread_mutex_unlock(&g_stack_lock);
    if (s) munmap(s, CO_STACK);
}
#endif

/* a new coroutine's stack, ready to start in mim_co_entry */
static void co_stack_new(mim_co *co) {
#ifdef _WIN32
    co->sp = CreateFiberEx((SIZE_T)64 << 10, CO_STACK, FIBER_FLAG_FLOAT_SWITCH, co_fiber_main, co);
    if (!co->sp) mim_panic("out of memory (coroutine stack)");
#else
    co->stack = stack_get();
    co->sp = co_first_frame(co->stack + CO_STACK, co);
#endif
}

/* free a finished coroutine's stack (from its scheduler, off that stack) */
static void co_stack_free(mim_co *co) {
#ifdef _WIN32
    DeleteFiber(co->sp);
    co->sp = NULL;
#else
    stack_put(co->stack);
    co->stack = NULL;
#endif
}

static void co_ready(mim_co *co) {
    co_sched *s = co->home;
    pthread_mutex_lock(&s->m);
    co->next = NULL;
    if (s->tail) s->tail->next = co;
    else s->head = co;
    s->tail = co;
    pthread_cond_signal(&s->c);
    pthread_mutex_unlock(&s->m);
}

/* switch back to the scheduler; it runs `fn(co, arg)` once this coroutine
 * is off its stack, so a wakeup can never come too early */
static void co_park(co_park_fn fn, void *arg) {
    mim_co *co = t_co;
    co->park = fn;
    co->park_arg = arg;
    mim_co_switch(&co->sp, co->home->sp);
}

static void yield_park(mim_co *co, void *arg) {
    (void)arg;
    co_ready(co);
}

/* let the other coroutines of this thread run */
static void co_yield(void) { co_park(yield_park, NULL); }

void mim_co_entry(mim_co *co) {
    co->fn(co->arg);
    co->finished = 1;
    mim_co_switch(&co->sp, co->home->sp);
    __builtin_unreachable();
}

static void co_finish(mim_co *co) {
    co_stack_free(co);
    pthread_mutex_lock(&co->m);
    co->done = 1;
    mim_co *j = co->joiner;
    int detached = co->detached;
    pthread_cond_broadcast(&co->c);
    pthread_mutex_unlock(&co->m);
    if (detached) {
        pthread_mutex_destroy(&co->m);
        pthread_cond_destroy(&co->c);
        free(co);
    }
    if (j) co_ready(j);
}

static void *sched_main(void *p) {
    co_sched *s = (co_sched *)p;
#ifdef _WIN32
    s->sp = ConvertThreadToFiberEx(NULL, FIBER_FLAG_FLOAT_SWITCH); /* the scheduler's own fiber */
    if (!s->sp) mim_panic("cannot start the event loop");
#endif
    for (;;) {
        pthread_mutex_lock(&s->m);
        while (!s->head) pthread_cond_wait(&s->c, &s->m);
        mim_co *co = s->head;
        s->head = co->next;
        if (!s->head) s->tail = NULL;
        pthread_mutex_unlock(&s->m);

        int32_t depth = t_native_depth;
        int par = t_in_parallel;
        t_native_depth = co->native_depth;
        t_in_parallel = co->in_parallel;
        t_co = co;
        mim_co_switch(&s->sp, co->sp);
        t_co = NULL;
        co->native_depth = t_native_depth;
        co->in_parallel = t_in_parallel;
        t_native_depth = depth;
        t_in_parallel = par;

        if (co->finished) {
            co_finish(co);
        } else if (co->park) {
            co_park_fn fn = co->park;
            co->park = NULL;
            fn(co, co->park_arg);
        }
    }
    return NULL;
}

/* ---- the poller: sockets and timers ------------------------------------- */

/* One wait of a parked coroutine: for a descriptor, for a deadline, or for
 * whichever comes first. It lives on the waiting coroutine's stack; the
 * poller and the timer heap drop every reference to it (under g_wait_lock)
 * before the coroutine is woken. */
typedef struct co_wait {
    mim_co *co;
    int64_t at;   /* deadline (monotonic ns), or 0 */
    int64_t heap; /* position in the timer heap, or -1 */
    int fd, write;
    int dupfd;    /* the descriptor the poller watches, or -1 (Windows: 1 while registered) */
    int timed_out;
    struct co_wait *link; /* Windows: the next wait on the same socket */
    void *reg;            /* Windows: the socket's registration */
} co_wait;

#ifdef _WIN32
/* wepoll watches a socket once: each socket has one registration, with the
 * waits for reading and for writing on it. A registration no wait needs is
 * removed, and freed by the poller before its next wait (so an event already
 * returned for it never points at freed memory). */
typedef struct ws_reg {
    uintptr_t sock;
    co_wait *rd, *wr;
    struct ws_reg *next; /* in its hash bucket, then in the free list */
    int added, dead;
} ws_reg;
#define WS_BUCKETS 4096
static ws_reg *g_regs[WS_BUCKETS];
static ws_reg *g_reg_free;
static HANDLE g_ep;
static SOCKET g_wake_sock = INVALID_SOCKET;
static struct sockaddr_in g_wake_addr;
#else
static int g_poll = -1;
static int g_wake[2] = {-1, -1};
#endif
static pthread_mutex_t g_wait_lock = PTHREAD_MUTEX_INITIALIZER;
static co_wait **g_timers; /* a binary min-heap on `at` */
static int64_t g_ntimers, g_timers_cap;

static int64_t mono_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void heap_set(int64_t i, co_wait *w) {
    g_timers[i] = w;
    w->heap = i;
}

static void heap_up(int64_t i) {
    co_wait *w = g_timers[i];
    while (i > 0 && g_timers[(i - 1) / 2]->at > w->at) {
        heap_set(i, g_timers[(i - 1) / 2]);
        i = (i - 1) / 2;
    }
    heap_set(i, w);
}

static void heap_down(int64_t i) {
    co_wait *w = g_timers[i];
    for (;;) {
        int64_t c = 2 * i + 1;
        if (c >= g_ntimers) break;
        if (c + 1 < g_ntimers && g_timers[c + 1]->at < g_timers[c]->at) c++;
        if (g_timers[c]->at >= w->at) break;
        heap_set(i, g_timers[c]);
        i = c;
    }
    heap_set(i, w);
}

static void timer_push(co_wait *w) {
    if (g_ntimers == g_timers_cap) {
        g_timers_cap = g_timers_cap ? g_timers_cap * 2 : 64;
        g_timers = (co_wait **)realloc(g_timers, sizeof(co_wait *) * (size_t)g_timers_cap);
    }
    g_timers[g_ntimers] = w;
    w->heap = g_ntimers++;
    heap_up(w->heap);
}

static void timer_remove(co_wait *w) {
    int64_t i = w->heap;
    w->heap = -1;
    co_wait *last = g_timers[--g_ntimers];
    if (i == g_ntimers) return;
    heap_set(i, last);
    heap_up(i);
    heap_down(last->heap);
}

static void poller_wake(void) {
#ifdef _WIN32
    char b = 1;
    sendto(g_wake_sock, &b, 1, 0, (struct sockaddr *)&g_wake_addr, sizeof g_wake_addr);
#else
    char b = 1;
    ssize_t r = write(g_wake[1], &b, 1);
    (void)r;
#endif
}

#ifdef _WIN32
/* the registration of a socket (under g_wait_lock) */
static ws_reg *reg_of(uintptr_t sock) {
    ws_reg **b = &g_regs[(sock >> 2) % WS_BUCKETS];
    for (ws_reg *r = *b; r; r = r->next)
        if (r->sock == sock) return r;
    ws_reg *r = (ws_reg *)calloc(1, sizeof *r);
    r->sock = sock;
    r->next = *b;
    *b = r;
    return r;
}

/* watch the socket for what its waits need, or stop watching it when none
 * is left (under g_wait_lock); 0, or -1 when it cannot be watched */
static int reg_arm(ws_reg *r) {
    uint32_t want = (r->rd ? EPOLLIN | EPOLLRDHUP : 0) | (r->wr ? EPOLLOUT : 0);
    if (!want) {
        if (r->added) epoll_ctl(g_ep, EPOLL_CTL_DEL, (SOCKET)r->sock, NULL);
        ws_reg **b = &g_regs[(r->sock >> 2) % WS_BUCKETS];
        while (*b != r) b = &(*b)->next;
        *b = r->next;
        r->dead = 1;
        r->next = g_reg_free;
        g_reg_free = r;
        return 0;
    }
    struct epoll_event e;
    memset(&e, 0, sizeof e);
    e.events = want | EPOLLONESHOT;
    e.data.ptr = r;
    /* a socket closed and reused since: wepoll forgot it, so add it again */
    if (r->added && epoll_ctl(g_ep, EPOLL_CTL_MOD, (SOCKET)r->sock, &e) == 0) return 0;
    if (epoll_ctl(g_ep, EPOLL_CTL_ADD, (SOCKET)r->sock, &e) != 0) return -1;
    r->added = 1;
    return 0;
}

/* take w out of its socket's list (under g_wait_lock) */
static void reg_unlink(co_wait *w) {
    ws_reg *r = (ws_reg *)w->reg;
    for (co_wait **p = w->write ? &r->wr : &r->rd; *p; p = &(*p)->link) {
        if (*p == w) {
            *p = w->link;
            break;
        }
    }
    w->reg = NULL;
}
#endif

/* stop watching w's descriptor (under g_wait_lock) */
static void fd_unwatch(co_wait *w) {
    if (w->dupfd < 0) return;
#ifdef _WIN32
    ws_reg *r = (ws_reg *)w->reg;
    reg_unlink(w);
    reg_arm(r);
#else
#ifndef __APPLE__
    epoll_ctl(g_poll, EPOLL_CTL_DEL, w->dupfd, NULL);
#endif
    close(w->dupfd); /* a kqueue forgets a closed descriptor by itself */
#endif
    w->dupfd = -1;
}

/* wake the waits whose deadline has passed; the time until the next one, or -1 */
static int64_t timers_due(void) {
    mim_co *due = NULL;
    int64_t wait = -1, now = mono_ns();
    pthread_mutex_lock(&g_wait_lock);
    while (g_ntimers && g_timers[0]->at <= now) {
        co_wait *w = g_timers[0];
        timer_remove(w);
        if (w->fd >= 0) {
            w->timed_out = 1;
            fd_unwatch(w);
        }
        w->co->next = due;
        due = w->co;
    }
    if (g_ntimers) wait = g_timers[0]->at - now;
    pthread_mutex_unlock(&g_wait_lock);
    while (due) {
        mim_co *n = due->next;
        co_ready(due);
        due = n;
    }
    return wait;
}

static void fd_fired(co_wait *w) {
    pthread_mutex_lock(&g_wait_lock);
    mim_co *co = w->co;
    if (w->heap >= 0) timer_remove(w);
    fd_unwatch(w);
    pthread_mutex_unlock(&g_wait_lock);
    co_ready(co);
}

#ifdef _WIN32
/* events on a registration: wake the waits they answer, watch on for the rest */
static void reg_fired(ws_reg *r, uint32_t ev) {
    mim_co *due = NULL;
    pthread_mutex_lock(&g_wait_lock);
    if (!r->dead) {
        co_wait *woken = NULL;
        if (ev & (EPOLLIN | EPOLLRDHUP | EPOLLHUP | EPOLLERR)) {
            woken = r->rd;
            r->rd = NULL;
        }
        if (ev & (EPOLLOUT | EPOLLHUP | EPOLLERR)) {
            co_wait *w = r->wr;
            r->wr = NULL;
            while (w) {
                co_wait *n = w->link;
                w->link = woken;
                woken = w;
                w = n;
            }
        }
        while (woken) {
            co_wait *w = woken;
            woken = w->link;
            if (w->heap >= 0) timer_remove(w);
            w->dupfd = -1;
            w->reg = NULL;
            w->co->next = due;
            due = w->co;
        }
        reg_arm(r); /* one-shot: watch again for the waits still there */
    }
    pthread_mutex_unlock(&g_wait_lock);
    while (due) {
        mim_co *n = due->next;
        co_ready(due);
        due = n;
    }
}
#endif

static void *poller_main(void *p) {
    (void)p;
    for (;;) {
        int64_t wait = timers_due();
#ifdef _WIN32
        /* registrations dropped before this wait cannot show up in it */
        pthread_mutex_lock(&g_wait_lock);
        ws_reg *gone = g_reg_free;
        g_reg_free = NULL;
        pthread_mutex_unlock(&g_wait_lock);
        while (gone) {
            ws_reg *n = gone->next;
            free(gone);
            gone = n;
        }
        struct epoll_event ev[64];
        int ms = wait < 0 ? -1 : (int)((wait + 999999) / 1000000);
        int n = epoll_wait(g_ep, ev, 64, ms);
        for (int i = 0; i < n; i++) {
            if (ev[i].data.ptr == NULL) {
                char buf[64];
                while (recv(g_wake_sock, buf, sizeof buf, 0) > 0) {
                }
            } else {
                reg_fired((ws_reg *)ev[i].data.ptr, ev[i].events);
            }
        }
#elif defined(__APPLE__)
        struct kevent ev[64];
        struct timespec ts, *tp = NULL;
        if (wait >= 0) {
            ts.tv_sec = (time_t)(wait / 1000000000);
            ts.tv_nsec = (long)(wait % 1000000000);
            tp = &ts;
        }
        int n = kevent(g_poll, NULL, 0, ev, 64, tp);
        for (int i = 0; i < n; i++) {
            if (ev[i].udata == NULL) {
                char buf[64];
                while (read(g_wake[0], buf, sizeof buf) > 0) {
                }
            } else {
                fd_fired((co_wait *)ev[i].udata);
            }
        }
#else
        struct epoll_event ev[64];
        int ms = wait < 0 ? -1 : (int)((wait + 999999) / 1000000);
        int n = epoll_wait(g_poll, ev, 64, ms);
        for (int i = 0; i < n; i++) {
            if (ev[i].data.ptr == NULL) {
                char buf[64];
                while (read(g_wake[0], buf, sizeof buf) > 0) {
                }
            } else {
                fd_fired((co_wait *)ev[i].data.ptr);
            }
        }
#endif
    }
    return NULL;
}

static void start_thread(void *(*fn)(void *), void *arg) {
    pthread_t t;
    pthread_attr_t a;
    pthread_attr_init(&a);
    pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
    pthread_attr_setstacksize(&a, (size_t)32 << 20); /* native code runs here too */
    if (pthread_create(&t, &a, fn, arg) != 0) mim_panic("cannot start the event loop");
    pthread_attr_destroy(&a);
}

/* is the event loop on? starts it on first use */
static int green_on(void) {
    int g = __atomic_load_n(&g_green, __ATOMIC_ACQUIRE);
    if (g >= 0) return g;
    pthread_mutex_lock(&g_green_lock);
    if (g_green < 0) {
        const char *off = getenv("MIMIC_NO_EVENT_LOOP");
        int on = !(off && *off && strcmp(off, "0") != 0);
#ifdef _WIN32
        if (on) {
            WSADATA d;
            on = WSAStartup(MAKEWORD(2, 2), &d) == 0;
        }
        if (on) {
            g_ep = epoll_create1(0);
            /* the poller's wake-up: a loopback datagram to itself */
            g_wake_sock = WSASocketW(AF_INET, SOCK_DGRAM, IPPROTO_UDP, NULL, 0, WSA_FLAG_NO_HANDLE_INHERIT);
            memset(&g_wake_addr, 0, sizeof g_wake_addr);
            g_wake_addr.sin_family = AF_INET;
            g_wake_addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
            int len = sizeof g_wake_addr;
            u_long nb = 1;
            on = g_ep != NULL && g_wake_sock != INVALID_SOCKET &&
                 bind(g_wake_sock, (struct sockaddr *)&g_wake_addr, sizeof g_wake_addr) == 0 &&
                 getsockname(g_wake_sock, (struct sockaddr *)&g_wake_addr, &len) == 0 && ioctlsocket(g_wake_sock, FIONBIO, &nb) == 0;
            if (on) {
                struct epoll_event e;
                memset(&e, 0, sizeof e);
                e.events = EPOLLIN;
                e.data.ptr = NULL;
                on = epoll_ctl(g_ep, EPOLL_CTL_ADD, g_wake_sock, &e) == 0;
            }
        }
        if (on) {
#else
        if (on) {
            g_page = (size_t)sysconf(_SC_PAGESIZE);
#ifdef __APPLE__
            g_poll = kqueue();
#else
            g_poll = epoll_create1(EPOLL_CLOEXEC);
#endif
            on = g_poll >= 0 && pipe(g_wake) == 0;
        }
        if (on) {
            fcntl(g_wake[0], F_SETFL, O_NONBLOCK);
            fcntl(g_wake[0], F_SETFD, FD_CLOEXEC);
            fcntl(g_wake[1], F_SETFD, FD_CLOEXEC);
#ifdef __APPLE__
            fcntl(g_poll, F_SETFD, FD_CLOEXEC);
            struct kevent kev;
            EV_SET(&kev, g_wake[0], EVFILT_READ, EV_ADD, 0, 0, NULL);
            kevent(g_poll, &kev, 1, NULL, 0, NULL);
#else
            struct epoll_event e = {0};
            e.events = EPOLLIN;
            e.data.ptr = NULL;
            epoll_ctl(g_poll, EPOLL_CTL_ADD, g_wake[0], &e);
#endif
#endif /* _WIN32 */
            g_nsched = g_nthreads > 1 ? g_nthreads : 2;
            g_sched = (co_sched *)calloc((size_t)g_nsched, sizeof(co_sched));
            for (int i = 0; i < g_nsched; i++) {
                pthread_mutex_init(&g_sched[i].m, NULL);
                pthread_cond_init(&g_sched[i].c, NULL);
                start_thread(sched_main, &g_sched[i]);
            }
            start_thread(poller_main, NULL);
        }
        __atomic_store_n(&g_green, on, __ATOMIC_RELEASE);
    }
    pthread_mutex_unlock(&g_green_lock);
    return g_green;
}

/* start fn(arg) as a coroutine; a detached one cleans up after itself,
 * any other must be joined (co_join) */
static mim_co *co_spawn(void (*fn)(void *), void *arg, int detached) {
    mim_co *co = (mim_co *)calloc(1, sizeof(mim_co));
    if (!co) mim_panic("out of memory");
    co->fn = fn;
    co->arg = arg;
    co->detached = detached;
    co_stack_new(co);
    pthread_mutex_init(&co->m, NULL);
    pthread_cond_init(&co->c, NULL);
    uint32_t k = __atomic_fetch_add(&g_next_sched, 1, __ATOMIC_RELAXED);
    co->home = &g_sched[k % (uint32_t)g_nsched];
    co_ready(co);
    return co;
}

static void join_park(mim_co *self, void *arg) {
    mim_co *co = (mim_co *)arg;
    pthread_mutex_lock(&co->m);
    int done = co->done;
    if (!done) co->joiner = self;
    pthread_mutex_unlock(&co->m);
    if (done) co_ready(self);
}

/* wait for a coroutine to finish (from a coroutine or a thread); frees it */
static void co_join(mim_co *co) {
    if (t_co) {
        co_park(join_park, co);
    } else {
        pthread_mutex_lock(&co->m);
        while (!co->done) pthread_cond_wait(&co->c, &co->m);
        pthread_mutex_unlock(&co->m);
    }
    pthread_mutex_lock(&co->m); /* co_finish may still be unlocking */
    pthread_mutex_unlock(&co->m);
    pthread_mutex_destroy(&co->m);
    pthread_cond_destroy(&co->c);
    free(co);
}

static void wait_park(mim_co *co, void *arg) {
    co_wait *w = (co_wait *)arg;
    w->co = co;
    int ready = 0, first = 0;
    pthread_mutex_lock(&g_wait_lock);
    if (w->at) {
        timer_push(w);
        first = w->heap == 0;
    }
#ifdef _WIN32
    if (w->fd >= 0) {
        ws_reg *r = reg_of((uintptr_t)w->fd);
        co_wait **list = w->write ? &r->wr : &r->rd;
        w->link = *list;
        *list = w;
        w->reg = r;
        w->dupfd = 1;
        if (reg_arm(r) != 0) { /* let the caller's next call report the error */
            reg_unlink(w);
            w->dupfd = -1;
            reg_arm(r);
            if (w->heap >= 0) timer_remove(w);
            ready = 1;
        }
    }
#else
    if (w->fd >= 0) {
        /* each wait watches its own duplicate of the descriptor, so a reader
         * and a writer of one socket can wait at the same time */
        int d = fcntl(w->fd, F_DUPFD_CLOEXEC, 0); /* never inherited by os::run */
        int ok = d >= 0;
        if (ok) {
#ifdef __APPLE__
            struct kevent kev;
            EV_SET(&kev, d, w->write ? EVFILT_WRITE : EVFILT_READ, EV_ADD | EV_ONESHOT, 0, 0, w);
            ok = kevent(g_poll, &kev, 1, NULL, 0, NULL) == 0;
#else
            struct epoll_event e = {0};
            e.events = (w->write ? EPOLLOUT : EPOLLIN) | EPOLLONESHOT | EPOLLRDHUP;
            e.data.ptr = w;
            ok = epoll_ctl(g_poll, EPOLL_CTL_ADD, d, &e) == 0;
#endif
            if (ok) w->dupfd = d;
            else close(d);
        }
        if (!ok) { /* let the caller's next call report the error */
            if (w->heap >= 0) timer_remove(w);
            ready = 1;
        }
    }
#endif
    pthread_mutex_unlock(&g_wait_lock);
    if (first) poller_wake();
    if (ready) co_ready(co);
}

/* park until fd can be read (or written); 0 when timeout_ns (if > 0) passed first */
static int co_wait_fd(int fd, int write, int64_t timeout_ns) {
    co_wait w = {NULL, timeout_ns > 0 ? mono_ns() + timeout_ns : 0, -1, fd, write, -1, 0, NULL, NULL};
    co_park(wait_park, &w);
    return !w.timed_out;
}

static void co_sleep(int64_t ns) {
    co_wait w = {NULL, mono_ns() + ns, -1, -1, 0, -1, 0, NULL, NULL};
    co_park(wait_park, &w);
}

/* a lock that a coroutine waits for without blocking its thread */
static void co_lock(pthread_mutex_t *m) {
    if (!t_co) {
        pthread_mutex_lock(m);
        return;
    }
    while (pthread_mutex_trylock(m) != 0) co_yield();
}

/* the socket's receive or send timeout (SO_RCVTIMEO / SO_SNDTIMEO) in ns */
static int64_t sock_timeo(int fd, int which) {
#ifdef _WIN32
    DWORD ms = 0; /* Windows keeps socket timeouts in milliseconds */
    int len = sizeof ms;
    if (getsockopt((SOCKET)fd, SOL_SOCKET, which, (char *)&ms, &len) != 0) return 0;
    return (int64_t)ms * 1000000;
#else
    struct timeval tv = {0, 0};
    socklen_t len = sizeof tv;
    if (getsockopt(fd, SOL_SOCKET, which, &tv, &len) != 0) return 0;
    return (int64_t)tv.tv_sec * 1000000000 + (int64_t)tv.tv_usec * 1000;
#endif
}

/* recv and send that park instead of blocking inside a coroutine; the
 * socket's own timeouts still apply (Windows: mimic_socket.c) */
#ifndef _WIN32
static ssize_t mrecv(int fd, void *buf, size_t n) {
    if (!t_co) return recv(fd, buf, n, 0);
    for (;;) {
        ssize_t r = recv(fd, buf, n, MSG_DONTWAIT);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        if (!co_wait_fd(fd, 0, sock_timeo(fd, SO_RCVTIMEO))) {
            errno = EAGAIN; /* as a blocking socket reports its timeout */
            return -1;
        }
    }
}

static ssize_t msend(int fd, const void *buf, size_t n, int flags) {
    if (!t_co) return send(fd, buf, n, flags);
    for (;;) {
        ssize_t r = send(fd, buf, n, flags | MSG_DONTWAIT);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        if (!co_wait_fd(fd, 1, sock_timeo(fd, SO_SNDTIMEO))) {
            errno = EAGAIN;
            return -1;
        }
    }
}

static ssize_t mrecvfrom(int fd, void *buf, size_t n, struct sockaddr *from, socklen_t *fl) {
    if (!t_co) return recvfrom(fd, buf, n, 0, from, fl);
    for (;;) {
        ssize_t r = recvfrom(fd, buf, n, MSG_DONTWAIT, from, fl);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        if (!co_wait_fd(fd, 0, sock_timeo(fd, SO_RCVTIMEO))) {
            errno = EAGAIN;
            return -1;
        }
    }
}

static ssize_t msendto(int fd, const void *buf, size_t n, const struct sockaddr *to, socklen_t tl) {
    if (!t_co) return sendto(fd, buf, n, 0, to, tl);
    for (;;) {
        ssize_t r = sendto(fd, buf, n, MSG_DONTWAIT, to, tl);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        if (!co_wait_fd(fd, 1, sock_timeo(fd, SO_SNDTIMEO))) {
            errno = EAGAIN;
            return -1;
        }
    }
}

#endif /* !_WIN32 */

/* before a call that would block until fd is ready (accept) */
static void mim_wait_readable(int fd) {
    if (t_co) co_wait_fd(fd, 0, 0);
}

#else /* no event loop: threads and blocking calls */

static int green_on(void) { return 0; }
#if MIM_THREADS
static void co_lock(pthread_mutex_t *m) { pthread_mutex_lock(m); }
#endif
#if !defined(_WIN32) && !defined(__wasi__)
#include <sys/socket.h>
static ssize_t mrecv(int fd, void *buf, size_t n) { return recv(fd, buf, n, 0); }
static ssize_t msend(int fd, const void *buf, size_t n, int flags) { return send(fd, buf, n, flags); }
static ssize_t mrecvfrom(int fd, void *buf, size_t n, struct sockaddr *from, socklen_t *fl) { return recvfrom(fd, buf, n, 0, from, fl); }
static ssize_t msendto(int fd, const void *buf, size_t n, const struct sockaddr *to, socklen_t tl) { return sendto(fd, buf, n, 0, to, tl); }
#endif
static void mim_wait_readable(int fd) { (void)fd; }

#endif /* MIM_GREEN */
