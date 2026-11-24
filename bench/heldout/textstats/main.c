// Generates about 10 MB of text (random words, separated by spaces and now
// and then a newline) and prints the number of lines, words and characters,
// the longest line and the most common letter.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define SIZE 10000000

int main(void) {
    char *text = malloc(SIZE + 16);
    int64_t len = 0, x = 42;
    while (len < SIZE) {
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t wlen = x % 8 + 1;
        for (int i = 0; i < wlen; i++) {
            x = (x * 1103515245 + 12345) % 2147483648;
            text[len++] = (char)('a' + x % 26);
        }
        x = (x * 1103515245 + 12345) % 2147483648;
        text[len++] = (x % 10 == 0) ? '\n' : ' ';
    }
    int64_t lines = 0, words = 0, longest = 0, line_len = 0;
    int64_t letters[26] = {0};
    int in_word = 0;
    for (int64_t i = 0; i < len; i++) {
        char c = text[i];
        if (c == '\n') {
            lines++;
            if (line_len > longest) longest = line_len;
            line_len = 0;
        } else {
            line_len++;
        }
        if (c == ' ' || c == '\n') {
            in_word = 0;
        } else {
            if (!in_word) words++;
            in_word = 1;
        }
        if (c >= 'a' && c <= 'z') letters[c - 'a']++;
    }
    if (line_len > longest) longest = line_len;
    int best = 0;
    for (int k = 1; k < 26; k++)
        if (letters[k] > letters[best]) best = k;
    printf("lines %lld\n", (long long)lines);
    printf("words %lld\n", (long long)words);
    printf("characters %lld\n", (long long)len);
    printf("longest line %lld\n", (long long)longest);
    printf("most common letter %c (%lld)\n", 'a' + best, (long long)letters[best]);
    free(text);
    return 0;
}
