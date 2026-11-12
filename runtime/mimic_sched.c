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
 * socket (kqueue on Apple systems, epoll on Linux) and every timer, and puts
 * coroutines back on their run queue when they can go on.
 *
 * A coroutine always runs on the same scheduler thread (it is never moved),
 * so thread-local state and locks behave as in ordinary threads.
 *
 * Where there is no event loop (Windows, WASI, other CPUs, or the environment
 * variable MIMIC_NO_EVENT_LOOP=1), the same functions fall back to threads and
 * blocking calls. */

#if MIM_THREADS && !defined(_WIN32) && (defined(__APPLE__) || defined(__linux__)) && (defined(__aarch64__) || defined(__x86_64__))
#define MIM_GREEN 1
#else
#define MIM_GREEN 0
#endif

#if MIM_GREEN
#include <sys/mman.h>
#include <sys/socket.h>
#include <fcntl.h>
#include <time.h>
#ifdef __APPLE__
#include <sys/event.h>
#else
#include <sys/epoll.h>
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
void mim_co_switch(void **save, void *to);
void mim_co_boot(void);
void mim_co_entry(mim_co *co);

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
    int wait_fd;     /* the descriptor the poller watches for this coroutine */
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
    stack_put(co->stack);
    co->stack = NULL;
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

typedef struct {
    int64_t at; /* monotonic nanoseconds */
    mim_co *co;
} co_timer;

static int g_poll = -1;
static int g_wake[2] = {-1, -1};
static pthread_mutex_t g_timer_lock = PTHREAD_MUTEX_INITIALIZER;
static co_timer *g_timers; /* a binary min-heap on `at` */
static int64_t g_ntimers, g_timers_cap;

static int64_t mono_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void timer_push(co_timer t) {
    if (g_ntimers == g_timers_cap) {
        g_timers_cap = g_timers_cap ? g_timers_cap * 2 : 64;
        g_timers = (co_timer *)realloc(g_timers, sizeof(co_timer) * (size_t)g_timers_cap);
    }
    int64_t i = g_ntimers++;
    while (i > 0 && g_timers[(i - 1) / 2].at > t.at) {
        g_timers[i] = g_timers[(i - 1) / 2];
        i = (i - 1) / 2;
    }
    g_timers[i] = t;
}

static co_timer timer_pop(void) {
    co_timer top = g_timers[0], last = g_timers[--g_ntimers];
    int64_t i = 0;
    for (;;) {
        int64_t c = 2 * i + 1;
        if (c >= g_ntimers) break;
        if (c + 1 < g_ntimers && g_timers[c + 1].at < g_timers[c].at) c++;
        if (g_timers[c].at >= last.at) break;
        g_timers[i] = g_timers[c];
        i = c;
    }
    if (g_ntimers) g_timers[i] = last;
    return top;
}

static void poller_wake(void) {
    char b = 1;
    ssize_t r = write(g_wake[1], &b, 1);
    (void)r;
}

/* fire the timers that are due; the time until the next one, or -1 */
static int64_t timers_due(void) {
    mim_co *due = NULL;
    int64_t wait = -1, now = mono_ns();
    pthread_mutex_lock(&g_timer_lock);
    while (g_ntimers && g_timers[0].at <= now) {
        mim_co *co = timer_pop().co;
        co->next = due;
        due = co;
    }
    if (g_ntimers) wait = g_timers[0].at - now;
    pthread_mutex_unlock(&g_timer_lock);
    while (due) {
        mim_co *n = due->next;
        co_ready(due);
        due = n;
    }
    return wait;
}

static void fd_fired(mim_co *co) {
#ifndef __APPLE__
    epoll_ctl(g_poll, EPOLL_CTL_DEL, co->wait_fd, NULL);
#endif
    close(co->wait_fd); /* a kqueue forgets a closed descriptor by itself */
    co->wait_fd = -1;
    co_ready(co);
}

static void *poller_main(void *p) {
    (void)p;
    for (;;) {
        int64_t wait = timers_due();
#ifdef __APPLE__
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
                fd_fired((mim_co *)ev[i].udata);
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
                fd_fired((mim_co *)ev[i].data.ptr);
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
    co->wait_fd = -1;
    co->stack = stack_get();
    co->sp = co_first_frame(co->stack + CO_STACK, co);
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

typedef struct {
    int fd, write;
} fd_wait;

static void fd_park(mim_co *co, void *arg) {
    fd_wait *w = (fd_wait *)arg;
    /* each wait watches its own duplicate of the descriptor, so a reader and
     * a writer of one socket can wait at the same time */
    int d = fcntl(w->fd, F_DUPFD_CLOEXEC, 0); /* never inherited by os::run */
    if (d < 0) {
        co_ready(co);
        return;
    }
    co->wait_fd = d;
#ifdef __APPLE__
    struct kevent kev;
    EV_SET(&kev, d, w->write ? EVFILT_WRITE : EVFILT_READ, EV_ADD | EV_ONESHOT, 0, 0, co);
    if (kevent(g_poll, &kev, 1, NULL, 0, NULL) != 0) {
#else
    struct epoll_event e = {0};
    e.events = (w->write ? EPOLLOUT : EPOLLIN) | EPOLLONESHOT | EPOLLRDHUP;
    e.data.ptr = co;
    if (epoll_ctl(g_poll, EPOLL_CTL_ADD, d, &e) != 0) {
#endif
        close(d);
        co->wait_fd = -1;
        co_ready(co);
    }
}

/* park until fd can be read (or written) */
static void co_wait_fd(int fd, int write) {
    fd_wait w = {fd, write};
    co_park(fd_park, &w);
}

static void timer_park(mim_co *co, void *arg) {
    co_timer t = {*(int64_t *)arg, co};
    pthread_mutex_lock(&g_timer_lock);
    timer_push(t);
    int first = g_timers[0].co == co;
    pthread_mutex_unlock(&g_timer_lock);
    if (first) poller_wake();
}

static void co_sleep(int64_t ns) {
    int64_t at = mono_ns() + ns;
    co_park(timer_park, &at);
}

/* a lock that a coroutine waits for without blocking its thread */
static void co_lock(pthread_mutex_t *m) {
    if (!t_co) {
        pthread_mutex_lock(m);
        return;
    }
    while (pthread_mutex_trylock(m) != 0) co_yield();
}

/* recv and send that park instead of blocking inside a coroutine */
static ssize_t mrecv(int fd, void *buf, size_t n) {
    if (!t_co) return recv(fd, buf, n, 0);
    for (;;) {
        ssize_t r = recv(fd, buf, n, MSG_DONTWAIT);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        co_wait_fd(fd, 0);
    }
}

static ssize_t msend(int fd, const void *buf, size_t n, int flags) {
    if (!t_co) return send(fd, buf, n, flags);
    for (;;) {
        ssize_t r = send(fd, buf, n, flags | MSG_DONTWAIT);
        if (r >= 0) return r;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return r;
        co_wait_fd(fd, 1);
    }
}

/* before a call that would block until fd is ready (accept) */
static void mim_wait_readable(int fd) {
    if (t_co) co_wait_fd(fd, 0);
}

#else /* no event loop: threads and blocking calls */

static int green_on(void) { return 0; }
#if MIM_THREADS && !defined(_WIN32)
static void co_lock(pthread_mutex_t *m) { pthread_mutex_lock(m); }
#endif
#if !defined(_WIN32) && !defined(__wasi__)
#include <sys/socket.h>
static ssize_t mrecv(int fd, void *buf, size_t n) { return recv(fd, buf, n, 0); }
static ssize_t msend(int fd, const void *buf, size_t n, int flags) { return send(fd, buf, n, flags); }
#endif
static void mim_wait_readable(int fd) { (void)fd; }

#endif /* MIM_GREEN */
