// bigfact: 20,000! as limbs of 9 decimal digits, multiplied by 2 … 20,000
// one after another. Prints the number of decimal digits, their sum, and
// the number of trailing zeros.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define BASE 1000000000LL

int main(void) {
    int64_t *l = malloc(sizeof(int64_t) * 10000), m = 1;
    l[0] = 1;
    for (int64_t k = 2; k <= 20000; k++) {
        int64_t carry = 0;
        for (int64_t i = 0; i < m; i++) {
            int64_t v = l[i] * k + carry;
            l[i] = v % BASE;
            carry = v / BASE;
        }
        while (carry) {
            l[m++] = carry % BASE;
            carry /= BASE;
        }
    }
    int64_t digits = 9 * (m - 1), sum = 0, zeros = 0;
    for (int64_t x = l[m - 1]; x; x /= 10) digits++;
    for (int64_t i = 0; i < m; i++)
        for (int64_t x = l[i]; x; x /= 10) sum += x % 10;
    int64_t i = 0;
    while (l[i] == 0) i++;
    zeros = 9 * i;
    for (int64_t x = l[i]; x % 10 == 0; x /= 10) zeros++;
    printf("%lld %lld %lld\n", (long long)digits, (long long)sum, (long long)zeros);
    return 0;
}
