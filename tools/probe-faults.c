/* How does first touching fresh memory scale with threads on this system?
 * Writes 800 MB of freshly allocated memory with 1, 2 and 4 threads, then
 * writes it again (pages already there: memory speed alone).
 * Built and run by the speed-probe workflow. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <pthread.h>
#endif

#define N ((size_t)100000000)

typedef struct {
    int64_t *p;
    size_t lo, hi;
} part;

static double now(void) {
    struct timespec t;
    timespec_get(&t, TIME_UTC);
    return (double)t.tv_sec + t.tv_nsec / 1e9;
}

#ifdef _WIN32
static DWORD WINAPI fill(LPVOID a)
#else
static void *fill(void *a)
#endif
{
    part *w = (part *)a;
    for (size_t i = w->lo; i < w->hi; i++) w->p[i] = (int64_t)i;
    return 0;
}

static double run(int64_t *p, int threads) {
    part parts[8];
    double t = now();
#ifdef _WIN32
    HANDLE h[8];
#else
    pthread_t h[8];
#endif
    for (int k = 0; k < threads; k++) {
        parts[k] = (part){p, N / threads * k, k == threads - 1 ? N : N / threads * (k + 1)};
#ifdef _WIN32
        h[k] = CreateThread(NULL, 0, fill, &parts[k], 0, NULL);
#else
        pthread_create(&h[k], NULL, fill, &parts[k]);
#endif
    }
    for (int k = 0; k < threads; k++) {
#ifdef _WIN32
        WaitForSingleObject(h[k], INFINITE);
        CloseHandle(h[k]);
#else
        pthread_join(h[k], NULL);
#endif
    }
    return now() - t;
}

int main(void) {
    int counts[] = {1, 2, 4};
    for (int c = 0; c < 3; c++) {
        int64_t *p = (int64_t *)malloc(N * sizeof(int64_t));
        double first = run(p, counts[c]);
        double again = run(p, counts[c]);
        printf("faults: %d thread(s): first touch %.3f s, again %.3f s\n", counts[c], first, again);
        free(p);
    }
    return 0;
}
