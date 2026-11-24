// Run-length encode a generated string of a few distinct letters (each run
// written as the letter and its length, e.g. "a3b12"), decode it again,
// and print the encoded length and whether the round trip matches.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>

int main(void) {
    const int64_t runs = 1000000;
    int64_t x = 42;
    char *text = malloc((size_t)runs * 9 + 1);
    int64_t n = 0;
    for (int64_t r = 0; r < runs; r++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        char letter = "abcde"[x % 5];
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t len = 1 + x % 9;
        for (int64_t k = 0; k < len; k++) text[n++] = letter;
    }
    text[n] = '\0';

    // encode
    char *enc = malloc((size_t)n * 2 + 32);
    int64_t e = 0;
    int64_t i = 0;
    while (i < n) {
        int64_t j = i;
        while (j < n && text[j] == text[i]) j++;
        e += sprintf(enc + e, "%c%lld", text[i], (long long)(j - i));
        i = j;
    }
    enc[e] = '\0';

    // decode
    char *dec = malloc((size_t)n + 1);
    int64_t d = 0;
    int64_t p = 0;
    while (p < e) {
        char letter = enc[p++];
        int64_t count = 0;
        while (p < e && isdigit((unsigned char)enc[p])) count = count * 10 + (enc[p++] - '0');
        for (int64_t k = 0; k < count; k++) dec[d++] = letter;
    }
    dec[d] = '\0';

    printf("%lld\n%lld\n%s\n", (long long)n, (long long)e,
           (d == n && memcmp(dec, text, (size_t)n) == 0) ? "true" : "false");
    free(text);
    free(enc);
    free(dec);
    return 0;
}
