// Recursive merge sort of generated numbers; prints a checksum and whether
// the result is sorted.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 2000000

static void merge_sort(int64_t *a, int64_t *tmp, int n) {
    if (n <= 1) return;
    int mid = n / 2;
    merge_sort(a, tmp, mid);
    merge_sort(a + mid, tmp, n - mid);
    int i = 0, j = mid, k = 0;
    while (i < mid && j < n) {
        if (a[i] <= a[j]) tmp[k++] = a[i++];
        else tmp[k++] = a[j++];
    }
    while (i < mid) tmp[k++] = a[i++];
    while (j < n) tmp[k++] = a[j++];
    memcpy(a, tmp, n * sizeof *a);
}

int main(void) {
    int64_t *a = malloc(N * sizeof *a);
    int64_t *tmp = malloc(N * sizeof *tmp);
    int64_t x = 42;
    for (int i = 0; i < N; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        a[i] = x % 1000000;
    }
    merge_sort(a, tmp, N);
    int64_t sum = 0;
    for (int i = 0; i < N; i++) sum += a[i] * (i % 1000 + 1);
    int sorted = 1;
    for (int i = 1; i < N; i++)
        if (a[i - 1] > a[i]) sorted = 0;
    printf("%lld\n", (long long)sum);
    printf("%s\n", sorted ? "true" : "false");
    free(a);
    free(tmp);
    return 0;
}
