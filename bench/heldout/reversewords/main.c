// Reverse the order of the words in each of many generated sentences and
// print a checksum of the results: for each sentence, the sum of
// byte * (position + 1), added up over all sentences.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static const char *WORDS[25] = {
    "time", "year", "people", "way", "day", "man", "thing", "woman", "life", "child",
    "world", "school", "state", "family", "student", "group", "country", "problem", "hand", "part",
    "place", "case", "week", "company", "system"};

int main(void) {
    const int64_t count = 300000;
    int64_t x = 42;
    char **sentences = malloc(sizeof(char *) * (size_t)count);
    for (int64_t s = 0; s < count; s++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        int64_t nwords = 3 + x % 9;
        char buf[256];
        size_t len = 0;
        for (int64_t k = 0; k < nwords; k++) {
            x = (x * 1103515245 + 12345) % 2147483648;
            const char *w = WORDS[x % 25];
            if (k > 0) buf[len++] = ' ';
            size_t wl = strlen(w);
            memcpy(buf + len, w, wl);
            len += wl;
        }
        buf[len] = '\0';
        sentences[s] = malloc(len + 1);
        memcpy(sentences[s], buf, len + 1);
    }

    int64_t checksum = 0, total_len = 0;
    for (int64_t s = 0; s < count; s++) {
        // split into words
        char copy[256];
        strcpy(copy, sentences[s]);
        char *words[32];
        int nw = 0;
        for (char *t = strtok(copy, " "); t != NULL; t = strtok(NULL, " ")) words[nw++] = t;
        // join them in reverse order
        char out[256];
        size_t len = 0;
        for (int k = nw - 1; k >= 0; k--) {
            size_t wl = strlen(words[k]);
            memcpy(out + len, words[k], wl);
            len += wl;
            if (k > 0) out[len++] = ' ';
        }
        out[len] = '\0';
        for (size_t i = 0; i < len; i++) checksum += (int64_t)(unsigned char)out[i] * (int64_t)(i + 1);
        total_len += (int64_t)len;
        free(sentences[s]);
    }
    printf("%lld\n%lld\n", (long long)total_len, (long long)checksum);
    free(sentences);
    return 0;
}
