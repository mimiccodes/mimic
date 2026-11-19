// lzw: 4,000,000 bytes (letters a-e in runs, from splitmix64) compressed
// by LZW with 12-bit codes (the dictionary a table child[code * 256 +
// byte], frozen once full) and decompressed again (prefix, last byte,
// first byte and length per code). Prints the number of codes, a
// position-weighted checksum, and 1 when decompressing gives the bytes
// back.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define N 4000000
#define CODES 4096

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

int main(void) {
    uint8_t *data = malloc(N), *out = malloc(N);
    for (int64_t i = 0; i < N; i++) data[i] = (uint8_t)(97 + mix(i / 4 + mix(i / 64) % 3) % 5);
    int32_t *child = malloc(sizeof(int32_t) * CODES * 256);
    memset(child, 0xff, sizeof(int32_t) * CODES * 256);
    int64_t *codes = malloc(sizeof(int64_t) * N), m = 0, next = 256, w = data[0];
    for (int64_t i = 1; i < N; i++) {
        int64_t ch = data[i], k = child[w * 256 + ch];
        if (k >= 0) {
            w = k;
            continue;
        }
        codes[m++] = w;
        if (next < CODES) child[w * 256 + ch] = (int32_t)next++;
        w = ch;
    }
    codes[m++] = w;
    int64_t check = 0;
    for (int64_t i = 0; i < m; i++) check += codes[i] * (i % 251 + 1);
    // decoding: each code's prefix, last and first byte, and length
    int64_t prefix[CODES], last[CODES], first[CODES], len[CODES];
    for (int c = 0; c < 256; c++) {
        prefix[c] = -1;
        last[c] = first[c] = c;
        len[c] = 1;
    }
    next = 256;
    int64_t pos = 0, prev = -1;
    for (int64_t i = 0; i < m; i++) {
        int64_t c = codes[i];
        if (prev >= 0 && next < CODES) {
            prefix[next] = prev;
            last[next] = c < next ? first[c] : first[prev];
            first[next] = first[prev];
            len[next] = len[prev] + 1;
            next++;
        }
        // the string of c, written from its end
        for (int64_t k = c, at = pos + len[c] - 1; k >= 0; k = prefix[k]) out[at--] = (uint8_t)last[k];
        pos += len[c];
        prev = c;
    }
    int same = pos == N && memcmp(out, data, N) == 0;
    printf("%lld %lld %d\n", (long long)m, (long long)check, same);
    return 0;
}
