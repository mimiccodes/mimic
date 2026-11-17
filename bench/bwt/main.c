// bwt: 256 KiB of generated text in 32 blocks of 8 KiB; for each block the
// Burrows-Wheeler transform (rotations sorted by prefix doubling),
// move-to-front over the last column, and the block back from its
// transform. Prints the sum of the primary indices, a checksum of the
// transforms, the zeros move-to-front made, and 1 when every block came back.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define B 8192
static int64_t key[B];

static int by_key(const void *a, const void *b) {
    int x = *(const int *)a, y = *(const int *)b;
    return key[x] < key[y] ? -1 : key[x] > key[y] ? 1 : x - y;
}

int main(void) {
    const int blocks = 32, n = B * 32;
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
    int64_t primary = 0, check = 0, zeros = 0;
    int same = 1;
    static int sa[B], rank[B], tmp[B], order[B];
    static uint8_t last[B], out[B];
    for (int k = 0; k < blocks; k++) {
        const uint8_t *blk = d + (int64_t)k * B;
        for (int i = 0; i < B; i++) rank[i] = blk[i];
        for (int step = 1;; step *= 2) {
            for (int i = 0; i < B; i++) {
                key[i] = (int64_t)rank[i] * 1048576 + rank[(i + step) % B];
                sa[i] = i;
            }
            qsort(sa, B, sizeof(int), by_key);
            int g = 0;
            for (int j = 0; j < B; j++) {
                if (j > 0 && key[sa[j]] != key[sa[j - 1]]) g++;
                tmp[sa[j]] = g;
            }
            memcpy(rank, tmp, sizeof rank);
            if (g == B - 1 || step * 2 >= B) break;
        }
        for (int i = 0; i < B; i++) {
            key[i] = rank[i];
            sa[i] = i;
        }
        qsort(sa, B, sizeof(int), by_key);
        int p = 0;
        int64_t c = 0;
        for (int j = 0; j < B; j++) {
            last[j] = blk[(sa[j] + B - 1) % B];
            if (sa[j] == 0) p = j;
            c = (c * 31 + last[j] + j) % 1000000007;
        }
        primary += p;
        check = (check + c) % 1000000007;
        uint8_t table[256];
        for (int i = 0; i < 256; i++) table[i] = (uint8_t)i;
        for (int j = 0; j < B; j++) {
            int r = 0;
            while (table[r] != last[j]) r++;
            zeros += r == 0;
            memmove(table + 1, table, r);
            table[0] = last[j];
        }
        for (int j = 0; j < B; j++) {
            key[j] = last[j];
            order[j] = j;
        }
        qsort(order, B, sizeof(int), by_key);
        for (int j = 0, row = order[p]; j < B; j++) {
            out[j] = last[row];
            row = order[row];
        }
        same &= memcmp(out, blk, B) == 0;
    }
    printf("%lld %lld %lld %d\n", (long long)primary, (long long)check, (long long)zeros, same);
    free(d);
    return 0;
}
