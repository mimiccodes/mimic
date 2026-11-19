// anagram: 500,000 words of 3 to 8 letters from a-f (from splitmix64),
// written as one text and split again, then grouped into anagram classes:
// a word's class is its letter counts packed into one number (4 bits a
// letter), the numbers are sorted, and the classes are the runs. Prints
// the number of classes, the size of the largest, and the sum of the
// squares of the sizes.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 500000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int cmp(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

int main(void) {
    char *text = malloc(10 * (size_t)N), *p = text;
    for (int64_t k = 0; k < N; k++) {
        if (k) *p++ = ' ';
        for (int64_t j = 0, l = 3 + mix(k) % 6; j < l; j++) *p++ = (char)(97 + mix(16 * k + j) % 6);
    }
    *p = 0;
    int64_t *keys = malloc(sizeof(int64_t) * N), n = 0;
    for (char *q = text; *q;) {
        while (*q == ' ') q++;
        if (!*q) break;
        int64_t key = 0;
        for (; *q && *q != ' '; q++) key += (int64_t)1 << (4 * (*q - 97));
        keys[n++] = key;
    }
    qsort(keys, n, sizeof(int64_t), cmp);
    int64_t classes = 0, largest = 0, squares = 0;
    for (int64_t i = 0; i < n;) {
        int64_t j = i;
        while (j < n && keys[j] == keys[i]) j++;
        classes++;
        if (j - i > largest) largest = j - i;
        squares += (j - i) * (j - i);
        i = j;
    }
    printf("%lld %lld %lld\n", (long long)classes, (long long)largest, (long long)squares);
    return 0;
}
