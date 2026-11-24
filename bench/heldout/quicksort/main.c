// Quicksort of generated numbers (middle element as pivot, three-way
// partition); prints a checksum of the sorted list.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 2000000

static void quicksort(int64_t *a, int n) {
    if (n <= 1) return;
    int64_t p = a[n / 2];
    int lt = 0, i = 0, gt = n;
    while (i < gt) {
        if (a[i] < p) {
            int64_t t = a[lt]; a[lt] = a[i]; a[i] = t;
            lt++; i++;
        } else if (a[i] > p) {
            gt--;
            int64_t t = a[gt]; a[gt] = a[i]; a[i] = t;
        } else {
            i++;
        }
    }
    quicksort(a, lt);
    quicksort(a + gt, n - gt);
}

int main(void) {
    int64_t *a = malloc(N * sizeof *a);
    int64_t x = 42;
    for (int i = 0; i < N; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        a[i] = x % 1000000;
    }
    quicksort(a, N);
    int64_t sum = 0;
    for (int i = 0; i < N; i++) sum += a[i] * (i % 1000 + 1);
    printf("%lld\n", (long long)sum);
    free(a);
    return 0;
}
