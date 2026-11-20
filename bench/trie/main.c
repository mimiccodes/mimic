// trie: 200,000 generated words (4 to 14 letters of a 4-letter alphabet)
// put into a trie stored as flat arrays (four child slots per node, and
// whether a word ends there), then 200,000 other words looked up. Prints
// the trie's nodes, the lookups found as words and those found as
// prefixes.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int word(int64_t k, int8_t *w) {
    int n = 4 + (int)(mix(k * 31) % 11);
    for (int i = 0; i < n; i++) w[i] = (int8_t)(mix(k * 1000 + i) % 4);
    return n;
}

int main(void) {
    int64_t size = 1 + 200000 * 14;
    int32_t *kids = malloc(sizeof(int32_t) * size * 4);
    int8_t *ends = calloc(size, 1);
    for (int64_t i = 0; i < size * 4; i++) kids[i] = -1;
    int32_t n = 1;
    int8_t w[16];
    for (int64_t k = 0; k < 200000; k++) {
        int len = word(k, w), cur = 0;
        for (int i = 0; i < len; i++) {
            int at = cur * 4 + w[i];
            if (kids[at] < 0) kids[at] = n++;
            cur = kids[at];
        }
        ends[cur] = 1;
    }
    int64_t as_word = 0, as_prefix = 0;
    for (int64_t q = 0; q < 200000; q++) {
        int len = word(100000 + q, w), node = 0;
        for (int i = 0; i < len && node >= 0; i++) node = kids[node * 4 + w[i]];
        if (node >= 0) {
            as_prefix++;
            as_word += ends[node];
        }
    }
    printf("%d %lld %lld\n", n, (long long)as_word, (long long)as_prefix);
    free(kids); free(ends);
    return 0;
}
