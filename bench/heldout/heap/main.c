// A binary min-heap written by hand: push generated numbers, pop them all,
// and print a checksum of the popped order: the sum of value * (i % 1000 + 1)
// for the i-th value popped.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

static void push(int64_t *heap, int64_t *size, int64_t v) {
    int64_t i = (*size)++;
    heap[i] = v;
    while (i > 0) {
        int64_t p = (i - 1) / 2;
        if (heap[p] <= heap[i]) break;
        int64_t t = heap[p]; heap[p] = heap[i]; heap[i] = t;
        i = p;
    }
}

static int64_t pop(int64_t *heap, int64_t *size) {
    int64_t top = heap[0];
    int64_t n = --(*size);
    heap[0] = heap[n];
    int64_t i = 0;
    for (;;) {
        int64_t l = 2 * i + 1, r = 2 * i + 2, m = i;
        if (l < n && heap[l] < heap[m]) m = l;
        if (r < n && heap[r] < heap[m]) m = r;
        if (m == i) break;
        int64_t t = heap[m]; heap[m] = heap[i]; heap[i] = t;
        i = m;
    }
    return top;
}

int main(void) {
    const int64_t n = 1000000;
    int64_t x = 42;
    int64_t *heap = malloc(sizeof(int64_t) * (size_t)n);
    int64_t size = 0;
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        push(heap, &size, x % 1000000007);
    }
    int64_t checksum = 0, last = -1, sorted = 1;
    for (int64_t i = 0; i < n; i++) {
        int64_t v = pop(heap, &size);
        if (v < last) sorted = 0;
        last = v;
        checksum += v * (i % 1000 + 1);
    }
    printf("%lld\n%s\n", (long long)checksum, sorted ? "true" : "false");
    free(heap);
    return 0;
}
