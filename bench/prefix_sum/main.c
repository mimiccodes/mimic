#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    const int64_t n = 100000000;
    int64_t *s = malloc(sizeof(int64_t) * n), acc = 0;
    for (int64_t i = 0; i < n; i++) {
        acc += i % 1000;
        s[i] = acc;
    }
    printf("%lld %lld\n", (long long)s[n / 2], (long long)s[n - 1]);
    free(s);
    return 0;
}
