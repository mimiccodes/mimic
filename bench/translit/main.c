// translit: see main.mim.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const char *alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .";
    int64_t n = 8000000;
    char *text = malloc((size_t)n);
    for (int64_t k = 0; k < n; k++) text[k] = alphabet[(k * k * 7 + k * 13) % 64];
    char *out = malloc((size_t)n * 2);
    int64_t m = 0;
    for (int64_t i = 0; i < n; i++) {
        char c = text[i];
        if (c >= 'a' && c <= 'z') out[m++] = (char)('a' + (c - 'a' + 13) % 26);
        else if (c >= 'A' && c <= 'Z') out[m++] = (char)('A' + (c - 'A' + 13) % 26);
        else if (c >= '0' && c <= '9') { out[m++] = c; out[m++] = c; }
        else if (c == ' ') out[m++] = '_';
    }
    int64_t sum = 0;
    for (int64_t i = 0; i < m; i++) sum += (int64_t)(unsigned char)out[i] * (i % 251 + 1);
    printf("%lld\n%lld\n", (long long)m, (long long)sum);
    free(text);
    free(out);
    return 0;
}
