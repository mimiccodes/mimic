/* GPU kernels through Metal (macOS): fused loops over f32, i32, i64 and
 * truth values, compiled by the compiler to Metal source (codegen gpu.rs)
 * and run here on the GPU when the work is large enough. Metal is opened
 * with dlopen and driven through the Objective-C runtime (as the window
 * code drives AppKit): no link-time dependency, and programs run the same
 * without a GPU. Included at the end of mimic_rt.c.
 *
 *   MIMIC_GPU=0   never use the GPU;   MIMIC_GPU=1   always (when it can);
 *   otherwise     the GPU takes kernels of enough work (mim_gpu_wants). */

#if defined(__APPLE__) && MIM_THREADS && !defined(__wasi__) && TARGET_OS_OSX
#include <dlfcn.h>

typedef struct { unsigned long w, h, d; } gpu_size;

static struct {
    void *(*get_class)(const char *);
    void *(*sel)(const char *);
    void *msg;
    void *(*pool_push)(void);
    void (*pool_pop)(void *);
    void *device, *queue;
    int32_t state; /* 0 not tried, 1 ready, -1 none */
    int32_t mode;  /* MIMIC_GPU: -1 unset, 0 never, 1 always */
    int32_t lock;
} g_gpu = {0};

static double mim_now_s(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

#define GCLS(n) ((void *)g_gpu.get_class(n))
#define GSL(n) g_gpu.sel(n)
#define G0(R, o, s) ((R(*)(void *, void *))g_gpu.msg)((o), GSL(s))
#define G1(R, o, s, T1, a1) ((R(*)(void *, void *, T1))g_gpu.msg)((o), GSL(s), (a1))
#define G2(R, o, s, T1, a1, T2, a2) ((R(*)(void *, void *, T1, T2))g_gpu.msg)((o), GSL(s), (a1), (a2))
#define G3(R, o, s, T1, a1, T2, a2, T3, a3) ((R(*)(void *, void *, T1, T2, T3))g_gpu.msg)((o), GSL(s), (a1), (a2), (a3))

static void gpu_lock(void) {
    while (__atomic_exchange_n(&g_gpu.lock, 1, __ATOMIC_ACQUIRE)) {
    }
}
static void gpu_unlock(void) { __atomic_store_n(&g_gpu.lock, 0, __ATOMIC_RELEASE); }

static int gpu_ready(void) {
    int st = __atomic_load_n(&g_gpu.state, __ATOMIC_ACQUIRE);
    if (st) return st > 0;
    gpu_lock();
    if (!g_gpu.state) {
        const char *e = getenv("MIMIC_GPU");
        g_gpu.mode = e ? (e[0] == '0' ? 0 : 1) : -1;
        void *objc = dlopen("/usr/lib/libobjc.A.dylib", RTLD_LAZY | RTLD_GLOBAL);
        void *metal = g_gpu.mode == 0 ? NULL : dlopen("/System/Library/Frameworks/Metal.framework/Metal", RTLD_LAZY | RTLD_GLOBAL);
        void *(*create)(void) = metal ? (void *(*)(void))dlsym(metal, "MTLCreateSystemDefaultDevice") : NULL;
        if (objc && create) {
            *(void **)&g_gpu.get_class = dlsym(objc, "objc_getClass");
            *(void **)&g_gpu.sel = dlsym(objc, "sel_registerName");
            g_gpu.msg = dlsym(objc, "objc_msgSend");
            *(void **)&g_gpu.pool_push = dlsym(objc, "objc_autoreleasePoolPush");
            *(void **)&g_gpu.pool_pop = dlsym(objc, "objc_autoreleasePoolPop");
            g_gpu.device = create();
            if (g_gpu.device) g_gpu.queue = G0(void *, g_gpu.device, "newCommandQueue");
        }
        if (g_gpu.mode != 0 && getenv("MIMIC_GPU_DEBUG"))
            fprintf(stderr, "mimic: %s\n", g_gpu.queue ? "Metal GPU ready" : "no Metal GPU on this machine: running on the CPU cores");
        __atomic_store_n(&g_gpu.state, g_gpu.queue ? 1 : -1, __ATOMIC_RELEASE);
    }
    gpu_unlock();
    return g_gpu.state > 0;
}

/* Should a kernel of `n` elements and `cost` (an estimate of the work per
 * element, about one per operation) run on the GPU? The GPU reads and
 * writes the lists in place (unified memory), but starting Metal takes
 * ~0.1 s once, and the CPU's cores do simple arithmetic about as fast: the
 * GPU takes only much work (about a second of one core), then, once it is
 * running, a quarter of that. */
static int32_t g_gpu_used = 0;
int32_t mim_gpu_wants(int64_t n, int64_t cost) {
    if (n >= ((int64_t)1 << 32) || !gpu_ready()) return 0; /* (thread indices are 32-bit) */
    if (g_gpu.mode == 1) return n > 0;
    if (g_gpu.mode == 0) return 0;
    double work = (double)n * (double)cost;
    if (n < (1 << 20) || work < (__atomic_load_n(&g_gpu_used, __ATOMIC_RELAXED) ? 1e9 : 4e9)) return 0;
    __atomic_store_n(&g_gpu_used, 1, __ATOMIC_RELAXED);
    return 1;
}

/* Compiled pipelines, by kernel source (a constant string per kernel). */
#define GPU_PIPES 128
static struct { const char *src; void *pso; } g_pipes[GPU_PIPES];
static int32_t g_npipes = 0;

static void *gpu_pipeline(const char *src) {
    gpu_lock();
    for (int32_t i = 0; i < g_npipes; i++)
        if (g_pipes[i].src == src) {
            void *p = g_pipes[i].pso;
            gpu_unlock();
            return p;
        }
    void *pso = NULL;
    void *err = NULL;
    const char *dbg = getenv("MIMIC_GPU_DEBUG");
    if (dbg && dbg[0] == '2') fprintf(stderr, "%s\n", src);
    void *source = G1(void *, GCLS("NSString"), "stringWithUTF8String:", const char *, src);
    /* IEEE results, as on the CPU: no fast math */
    void *opts = G0(void *, G0(void *, GCLS("MTLCompileOptions"), "alloc"), "init");
    if (opts) G1(void, opts, "setFastMathEnabled:", signed char, 0);
    void *lib = G3(void *, g_gpu.device, "newLibraryWithSource:options:error:", void *, source, void *, opts, void **, &err);
    if (opts) G0(void, opts, "release");
    if (lib) {
        void *fname = G1(void *, GCLS("NSString"), "stringWithUTF8String:", const char *, "k");
        void *fn = G1(void *, lib, "newFunctionWithName:", void *, fname);
        if (fn) {
            pso = G2(void *, g_gpu.device, "newComputePipelineStateWithFunction:error:", void *, fn, void **, &err);
            G0(void, fn, "release");
        }
        G0(void, lib, "release");
    } else if (getenv("MIMIC_GPU_DEBUG") && err) {
        void *d = G0(void *, err, "localizedDescription");
        fprintf(stderr, "mimic: GPU kernel did not compile: %s\n", G0(const char *, d, "UTF8String"));
    }
    if (g_npipes < GPU_PIPES) {
        g_pipes[g_npipes].src = src;
        g_pipes[g_npipes].pso = pso;
        g_npipes++;
    }
    gpu_unlock();
    return pso;
}

/* Run kernel `src` over n elements: inputs `in` (nin buffers of in_bytes),
 * 8-byte arguments, the result into `out` (out_bytes). 1 when it ran. */
/* A buffer over `bytes` at `p`: the memory itself where Metal takes it
 * (whole pages around it, unified memory: nothing copied), else a copy.
 * `*off`: where p starts in it; `*own`: 1 when it is the memory itself. */
static void *gpu_wrap(void *p, int64_t bytes, unsigned long *off, int *own) {
    unsigned long page = (unsigned long)getpagesize();
    uintptr_t a = (uintptr_t)p, lo = a & ~(uintptr_t)(page - 1);
    uintptr_t hi = (a + (uintptr_t)(bytes ? bytes : 1) + page - 1) & ~(uintptr_t)(page - 1);
    void *b = ((void *(*)(void *, void *, void *, unsigned long, unsigned long, void *))g_gpu.msg)(g_gpu.device, GSL("newBufferWithBytesNoCopy:length:options:deallocator:"), (void *)lo, (unsigned long)(hi - lo), 0, NULL);
    if (b) {
        *off = (unsigned long)(a - lo);
        *own = 1;
        return b;
    }
    *off = 0;
    *own = 0;
    return G3(void *, g_gpu.device, "newBufferWithBytes:length:options:", const void *, p, unsigned long, (unsigned long)(bytes ? bytes : 8), unsigned long, 0);
}

int32_t mim_gpu_run(const char *src, int64_t n, int32_t nin, void *const *in, const int64_t *in_bytes, const int64_t *args, int32_t nargs, void *out, int64_t out_bytes) {
    if (!gpu_ready() || n <= 0 || nin > 60) return 0;
    double t0 = mim_now_s();
    void *pso = gpu_pipeline(src);
    double t1 = mim_now_s();
    if (!pso) return 0;
    void *pool = g_gpu.pool_push();
    void *bufs[64];
    unsigned long offs[64];
    int own;
    int32_t ok = 0;
    for (int32_t i = 0; i < nin; i++) bufs[i] = gpu_wrap(in[i], in_bytes[i], &offs[i], &own);
    int64_t zero = 0;
    void *argbuf = G3(void *, g_gpu.device, "newBufferWithBytes:length:options:", const void *, nargs ? (const void *)args : (const void *)&zero, unsigned long, (unsigned long)(nargs ? nargs * 8 : 8), unsigned long, 0);
    unsigned long out_off = 0;
    int out_own = 0;
    void *outbuf = gpu_wrap(out, out_bytes, &out_off, &out_own);
    if (argbuf && outbuf) {
        void *cmd = G0(void *, g_gpu.queue, "commandBuffer");
        void *enc = G0(void *, cmd, "computeCommandEncoder");
        G1(void, enc, "setComputePipelineState:", void *, pso);
        for (int32_t i = 0; i < nin; i++) G3(void, enc, "setBuffer:offset:atIndex:", void *, bufs[i], unsigned long, offs[i], unsigned long, (unsigned long)i);
        G3(void, enc, "setBuffer:offset:atIndex:", void *, outbuf, unsigned long, out_off, unsigned long, (unsigned long)nin);
        G3(void, enc, "setBuffer:offset:atIndex:", void *, argbuf, unsigned long, 0, unsigned long, (unsigned long)nin + 1);
        unsigned long tg = G0(unsigned long, pso, "maxTotalThreadsPerThreadgroup");
        if (tg > 256) tg = 256;
        gpu_size grid = {(unsigned long)n, 1, 1}, group = {tg, 1, 1};
        G2(void, enc, "dispatchThreads:threadsPerThreadgroup:", gpu_size, grid, gpu_size, group);
        G0(void, enc, "endEncoding");
        G0(void, cmd, "commit");
        G0(void, cmd, "waitUntilCompleted");
        long status = G0(long, cmd, "status"); /* MTLCommandBufferStatusCompleted = 4 */
        if (status == 4) {
            if (!out_own) memcpy(out, G0(void *, outbuf, "contents"), (size_t)out_bytes);
            ok = 1;
        }
        if (getenv("MIMIC_GPU_DEBUG")) fprintf(stderr, "mimic: GPU kernel over %lld elements: %s (compile %.1f ms, run %.1f ms%s)\n", (long long)n, ok ? "ran" : "failed", (t1 - t0) * 1e3, (mim_now_s() - t1) * 1e3, out_own ? "" : ", copied");
    }
    for (int32_t i = 0; i < nin; i++)
        if (bufs[i]) G0(void, bufs[i], "release");
    if (argbuf) G0(void, argbuf, "release");
    if (outbuf) G0(void, outbuf, "release");
    g_gpu.pool_pop(pool);
    return ok;
}

#else /* no Metal here: everything runs on the CPU */
int32_t mim_gpu_wants(int64_t n, int64_t cost) {
    (void)n;
    (void)cost;
    return 0;
}
int32_t mim_gpu_run(const char *src, int64_t n, int32_t nin, void *const *in, const int64_t *in_bytes, const int64_t *args, int32_t nargs, void *out, int64_t out_bytes) {
    (void)src, (void)n, (void)nin, (void)in, (void)in_bytes, (void)args, (void)nargs, (void)out, (void)out_bytes;
    return 0;
}
#endif
