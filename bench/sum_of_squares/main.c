#include <stdio.h>
#include <stdint.h>

int main(void) {
    int64_t s = 0;
    for (int64_t i = 0; i < 100000000; i++) {
        int64_t x = i % 1000;
        s += x * x;
    }
    printf("%lld\n", (long long)s);
    return 0;
}
