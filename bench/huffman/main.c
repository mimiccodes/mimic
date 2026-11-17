// huffman: 256 KiB of generated text Huffman-coded and decoded again (the
// tree merged by weight, then node number; canonical codes; packed into
// 64-bit words most significant bit first; decoded bit by bit). Prints the
// encoded size in bits, the longest code, a checksum of the packed words,
// and 1 when decoding gives the text back.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const int64_t n = 1 << 18;
    static const char *words[20] = {"the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector", "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"};
    uint8_t *d = malloc(n + 32);
    int64_t len = 0;
    for (int64_t i = 0; len < n; i++) {
        const char *w = words[i % 7 ? (i * 2654435761LL >> 13) % 20 : (i * 40503 >> 7) % 20];
        size_t k = strlen(w);
        memcpy(d + len, w, k);
        len += k;
        if (i % 11 == 10) d[len++] = '.';
        d[len++] = ' ';
    }
    int64_t count[256] = {0};
    for (int64_t i = 0; i < n; i++) count[d[i]]++;
    // the tree: repeatedly merge the two lightest (weight, node number)
    int64_t key[512], m = 0, parent[512], next = 256;
    for (int i = 0; i < 512; i++) parent[i] = -1;
    for (int b = 0; b < 256; b++) if (count[b]) key[m++] = count[b] * 1024 + b;
    while (m > 1) {
        int64_t k[2];
        for (int t = 0; t < 2; t++) {
            int at = 0;
            for (int j = 1; j < m; j++) if (key[j] < key[at]) at = j;
            k[t] = key[at];
            key[at] = key[--m];
        }
        parent[k[0] % 1024] = parent[k[1] % 1024] = next;
        key[m++] = (k[0] / 1024 + k[1] / 1024) * 1024 + next++;
    }
    int length[256] = {0}, maxlen = 0;
    for (int b = 0; b < 256; b++) {
        if (!count[b]) continue;
        for (int64_t q = b; parent[q] >= 0; q = parent[q]) length[b]++;
        if (length[b] > maxlen) maxlen = length[b];
    }
    // canonical codes in order of (length, byte)
    uint32_t code[256] = {0}, c = 0;
    int prev = 0;
    for (int l = 1; l <= maxlen; l++)
        for (int b = 0; b < 256; b++)
            if (count[b] && length[b] == l) {
                c <<= l - prev;
                code[b] = c++;
                prev = l;
            }
    int64_t bits = 0;
    for (int b = 0; b < 256; b++) bits += count[b] * length[b];
    int64_t nw = (bits + 63) / 64;
    uint64_t *w = calloc(nw + 1, 8);
    int64_t pos = 0;
    for (int64_t i = 0; i < n; i++) {
        int l = length[d[i]];
        uint64_t cc = code[d[i]];
        int64_t wi = pos / 64, off = pos % 64;
        if (off + l <= 64) {
            w[wi] |= cc << (64 - off - l);
        } else {
            int spill = (int)(off + l - 64);
            w[wi] |= cc >> spill;
            w[wi + 1] |= (cc & ((1u << spill) - 1)) << (64 - spill);
        }
        pos += l;
    }
    uint64_t check = 0;
    for (int64_t i = 0; i < nw; i++) check = (check * 31 + w[i] % 1000000007) % 1000000007;
    int16_t *table = malloc(sizeof(int16_t) * 16 * 32768);
    for (int i = 0; i < 16 * 32768; i++) table[i] = -1;
    for (int b = 0; b < 256; b++) if (count[b]) table[length[b] * 32768 + code[b]] = (int16_t)b;
    uint8_t *out = malloc(n);
    int64_t o = 0;
    uint32_t cur = 0;
    int l = 0;
    for (int64_t p = 0; p < bits; p++) {
        cur = cur << 1 | (uint32_t)(w[p / 64] >> (63 - p % 64) & 1);
        l++;
        int16_t hit = table[l * 32768 + cur];
        if (hit >= 0) {
            out[o++] = (uint8_t)hit;
            cur = 0;
            l = 0;
        }
    }
    printf("%lld %d %llu %d\n", (long long)bits, maxlen, (unsigned long long)check, o == n && memcmp(out, d, n) == 0);
    return 0;
}
