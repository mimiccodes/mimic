// varint: 4,000,000 readings of a counter (mostly small steps, every 97th
// a jump; from splitmix64), stored as differences, zigzag-mapped and
// written as LEB128 bytes, then decoded again. Prints the number of bytes,
// a position-weighted checksum, and 1 when decoding gives the values back.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 4000000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    int64_t *vals = malloc(sizeof(int64_t) * N), *back = malloc(sizeof(int64_t) * N);
    uint8_t *out = malloc(10 * N);
    int64_t v = 1000000;
    for (int64_t k = 0; k < N; k++) {
        v += k % 97 == 0 ? mix(k) % 1000000000 : mix(k) % 2001 - 1000;
        vals[k] = v;
    }
    int64_t len = 0, prev = 0;
    for (int64_t k = 0; k < N; k++) {
        int64_t d = vals[k] - prev;
        prev = vals[k];
        uint64_t z = (uint64_t)(d << 1) ^ (uint64_t)(d >> 63);
        while (z >= 128) {
            out[len++] = (uint8_t)(z & 127 | 128);
            z >>= 7;
        }
        out[len++] = (uint8_t)z;
    }
    int64_t check = 0;
    for (int64_t i = 0; i < len; i++) check += out[i] * (i % 251 + 1);
    int64_t m = 0, shift = 0;
    uint64_t z = 0;
    prev = 0;
    for (int64_t i = 0; i < len; i++) {
        z |= (uint64_t)(out[i] & 127) << shift;
        shift += 7;
        if (out[i] < 128) {
            prev += (int64_t)(z >> 1) ^ -(int64_t)(z & 1);
            back[m++] = prev;
            z = 0;
            shift = 0;
        }
    }
    int same = m == N;
    for (int64_t k = 0; same && k < N; k++) same = back[k] == vals[k];
    printf("%lld %lld %d\n", (long long)len, (long long)check, same);
    return 0;
}
