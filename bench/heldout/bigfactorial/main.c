// n! held as a list of decimal digits (lowest first), built by schoolbook
// multiplication; print its digit sum and number of digits.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    const int64_t n = 3000;
    int64_t cap = 100000;
    int64_t *digits = malloc(sizeof(int64_t) * cap);
    int64_t len = 1;
    digits[0] = 1;
    for (int64_t k = 2; k <= n; k++) {
        int64_t carry = 0;
        for (int64_t i = 0; i < len; i++) {
            int64_t v = digits[i] * k + carry;
            digits[i] = v % 10;
            carry = v / 10;
        }
        while (carry > 0) {
            digits[len++] = carry % 10;
            carry /= 10;
        }
    }
    int64_t sum = 0;
    for (int64_t i = 0; i < len; i++) sum += digits[i];
    printf("%lld\n%lld\n", (long long)sum, (long long)len);
    free(digits);
    return 0;
}
