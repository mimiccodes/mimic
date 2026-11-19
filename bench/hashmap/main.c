// hashmap: a hash table with open addressing and linear probing, 2,097,152
// slots: 1,000,000 keys (1 + mix(i) % 700,000, from splitmix64) go in, a
// repeat adding i % 100 to its value; then keys 1 + q % 1,400,000 for q
// below 2,000,000 are looked up. Prints the sum of the values found, how
// many lookups found their key, and the slots in use.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define SLOTS 2097152

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t find(const int64_t *keys, int64_t k) {
    int64_t s = mix(k * 31) & (SLOTS - 1);
    while (keys[s] != 0 && keys[s] != k) s = (s + 1) & (SLOTS - 1);
    return s;
}

int main(void) {
    int64_t n = 1000000;
    int64_t *keys = calloc(SLOTS, sizeof(int64_t)), *vals = calloc(SLOTS, sizeof(int64_t));
    for (int64_t i = 0; i < n; i++) {
        int64_t k = 1 + mix(i) % 700000, s = find(keys, k);
        keys[s] = k;
        vals[s] += i % 100;
    }
    int64_t total = 0, found = 0, used = 0;
    for (int64_t q = 0; q < 2 * n; q++) {
        int64_t k = 1 + q % 1400000, s = find(keys, k);
        if (keys[s] == k) { total += vals[s]; found++; }
    }
    for (int64_t s = 0; s < SLOTS; s++) used += keys[s] > 0;
    printf("%lld %lld %lld\n", (long long)total, (long long)found, (long long)used);
    free(keys);
    free(vals);
    return 0;
}
