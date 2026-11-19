// roman: 1,000,000 numbers (from splitmix64) written as Roman numerals
// (greedily) and read back. Prints the total length of the numerals, the
// sum of the values read back, and how many numerals contain "MM".
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static const int VALUES[13] = {1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1};
static const char *SYMBOLS[13] = {"M", "CM", "D", "CD", "C", "XC", "L", "XL", "X", "IX", "V", "IV", "I"};

static int digit(char c) {
    switch (c) {
    case 'I': return 1;
    case 'V': return 5;
    case 'X': return 10;
    case 'L': return 50;
    case 'C': return 100;
    case 'D': return 500;
    default: return 1000;
    }
}

int main(void) {
    int64_t lengths = 0, back = 0, mm = 0;
    char r[32];
    for (int64_t k = 0; k < 1000000; k++) {
        int64_t n = 1 + mix(k) % 3999;
        size_t l = 0;
        for (int i = 0; i < 13; i++)
            while (n >= VALUES[i]) {
                size_t sl = strlen(SYMBOLS[i]);
                memcpy(r + l, SYMBOLS[i], sl);
                l += sl;
                n -= VALUES[i];
            }
        r[l] = 0;
        lengths += (int64_t)l;
        int64_t t = 0;
        for (size_t i = 0; i < l; i++) {
            int v = digit(r[i]);
            t += i + 1 < l && v < digit(r[i + 1]) ? -v : v;
        }
        back += t;
        mm += strstr(r, "MM") != NULL;
    }
    printf("%lld %lld %lld\n", (long long)lengths, (long long)back, (long long)mm);
    return 0;
}
