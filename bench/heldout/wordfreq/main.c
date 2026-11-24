// Build a text of generated words drawn from a list of 50, count each
// word, and print the 10 most frequent with their counts.
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static const char *WORDS[50] = {
    "the", "of", "and", "to", "in", "is", "you", "that", "it", "he",
    "was", "for", "on", "are", "as", "with", "his", "they", "at", "be",
    "this", "have", "from", "or", "one", "had", "by", "word", "but", "not",
    "what", "all", "were", "we", "when", "your", "can", "said", "there", "use",
    "an", "each", "which", "she", "do", "how", "their", "if", "will", "up"};

int main(void) {
    const int64_t n = 1000000;
    int64_t x = 42;

    // build the text: words separated by single spaces
    char *text = malloc((size_t)n * 8 + 1);
    size_t len = 0;
    for (int64_t i = 0; i < n; i++) {
        x = (x * 1103515245 + 12345) % 2147483648;
        const char *w = WORDS[(x / 65536) % 50];
        if (i > 0) text[len++] = ' ';
        size_t wl = strlen(w);
        memcpy(text + len, w, wl);
        len += wl;
    }
    text[len] = '\0';

    // count: names in order of first appearance
    const char *names[50];
    int64_t counts[50];
    int distinct = 0;
    for (char *tok = strtok(text, " "); tok != NULL; tok = strtok(NULL, " ")) {
        int found = -1;
        for (int k = 0; k < distinct; k++) {
            if (strcmp(names[k], tok) == 0) { found = k; break; }
        }
        if (found < 0) {
            names[distinct] = tok;
            counts[distinct] = 0;
            found = distinct++;
        }
        counts[found]++;
    }

    // stable insertion sort by count, largest first (ties keep first-appearance order)
    for (int i = 1; i < distinct; i++) {
        const char *nm = names[i];
        int64_t c = counts[i];
        int j = i - 1;
        while (j >= 0 && counts[j] < c) {
            names[j + 1] = names[j];
            counts[j + 1] = counts[j];
            j--;
        }
        names[j + 1] = nm;
        counts[j + 1] = c;
    }
    for (int i = 0; i < 10 && i < distinct; i++) printf("%s %lld\n", names[i], (long long)counts[i]);
    free(text);
    return 0;
}
