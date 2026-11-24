// Encrypt a generated text with a Caesar shift of 13 (letters only),
// decrypt it again, and print a checksum of the cipher text and whether
// the round trip gives back the original.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static const char ALPHABET[] =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ .,!?;:";

static char shift(char c, int k) {
    if (c >= 'a' && c <= 'z') return (char)('a' + (c - 'a' + k) % 26);
    if (c >= 'A' && c <= 'Z') return (char)('A' + (c - 'A' + k) % 26);
    return c;
}

int main(void) {
    const int64_t n = 10000000;
    const int64_t alen = (int64_t)strlen(ALPHABET); // 59
    int64_t x = 42;
    char *text = malloc((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        text[i] = ALPHABET[x % alen];
    }
    text[n] = '\0';

    char *cipher = malloc((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) cipher[i] = shift(text[i], 13);
    cipher[n] = '\0';

    char *plain = malloc((size_t)n + 1);
    for (int64_t i = 0; i < n; i++) plain[i] = shift(cipher[i], 26 - 13);
    plain[n] = '\0';

    int64_t checksum = 0;
    for (int64_t i = 0; i < n; i++) checksum += (int64_t)(unsigned char)cipher[i] * (i % 255 + 1);

    printf("%lld\n%s\n", (long long)checksum, strcmp(text, plain) == 0 ? "true" : "false");
    free(text);
    free(cipher);
    free(plain);
    return 0;
}
