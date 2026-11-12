#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

int main(void) {
    const int64_t n = 10000000;
    int64_t *xs = malloc(sizeof(int64_t) * n);
    for (int64_t i = 0; i < n; i++) xs[i] = (i * 1103515245 + 12345) % 2147483648LL;
    qsort(xs, n, sizeof(int64_t), cmp);
    printf("%lld %lld %lld\n", (long long)xs[0], (long long)xs[n / 2], (long long)xs[n - 1]);
    free(xs);
    return 0;
}
