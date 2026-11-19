// lru: an LRU cache of 10,000 entries over 4,000,000 accesses to keys
// below 100,000, most of them to 2,000 hot keys (from splitmix64); the
// recency order is a doubly linked list through the keys. Prints the hits,
// the misses, and the sum of the keys left in the cache.
#include <stdint.h>
#include <stdio.h>

#define H 100000
#define CAP 10000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t prev[H + 1], next[H + 1];
static char inc[H + 1];

static void unlink_key(int64_t k) {
    next[prev[k]] = next[k];
    prev[next[k]] = prev[k];
}

static void append(int64_t k) {
    int64_t m = prev[H];
    next[m] = k;
    prev[k] = m;
    next[k] = H;
    prev[H] = k;
}

int main(void) {
    prev[H] = next[H] = H;
    int64_t size = 0, hits = 0, misses = 0;
    for (int64_t t = 0; t < 4000000; t++) {
        int64_t k = mix(t + 10000000) % 4 == 0 ? mix(t) % 100000 : mix(t) % 2000;
        if (inc[k]) {
            hits++;
            unlink_key(k);
            append(k);
        } else {
            misses++;
            if (size == CAP) {
                int64_t old = next[H];
                unlink_key(old);
                inc[old] = 0;
                size--;
            }
            inc[k] = 1;
            size++;
            append(k);
        }
    }
    int64_t sum = 0;
    for (int64_t k = 0; k < H; k++)
        if (inc[k]) sum += k;
    printf("%lld %lld %lld\n", (long long)hits, (long long)misses, (long long)sum);
    return 0;
}
