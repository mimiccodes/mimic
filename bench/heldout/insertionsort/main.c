// Insertion sort of generated numbers; prints a checksum of the sorted list.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 20000

int main(void) {
    int64_t *a = malloc(N * sizeof *a);
    int64_t x = 42;
    for (int i = 0; i < N; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        a[i] = x % 1000000;
    }
    for (int i = 1; i < N; i++) {
        int64_t v = a[i];
        int j = i - 1;
        while (j >= 0 && a[j] > v) {
            a[j + 1] = a[j];
            j--;
        }
        a[j + 1] = v;
    }
    int64_t sum = 0;
    for (int i = 0; i < N; i++) sum += a[i] * (i % 1000 + 1);
    printf("%lld\n", (long long)sum);
    free(a);
    return 0;
}
