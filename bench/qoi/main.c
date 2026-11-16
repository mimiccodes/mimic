/* qoi: a 1024x768 RGBA image encoded as QOI (qoiformat.org) and decoded
 * again, 5 times; prints the encoded size, a checksum of the bytes and
 * whether the decoded pixels equal the original. A pixel is one 32-bit
 * value r << 24 | g << 16 | b << 8 | a. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define W 1024
#define H 768

static uint32_t rgba(uint32_t r, uint32_t g, uint32_t b, uint32_t a) { return r << 24 | g << 16 | b << 8 | a; }
static uint32_t slot(uint32_t p) { return ((p >> 24) * 3 + (p >> 16 & 255) * 5 + (p >> 8 & 255) * 7 + (p & 255) * 11) % 64; }

static uint32_t pixel(long i) {
    long x = i % W, y = i / W;
    long noise = (i * 1103515245 + 12345) % 2147483648 / 65536;
    long band = y * 10 / H;
    if (band < 2) return rgba(40, 90, 200, 255);
    if (band < 5) return rgba(x * 3 % 256, y * 5 % 256, (x + y) % 256, 255);
    if (band < 7) { static const uint32_t c[3] = {0xff0000ff, 0x00ff00ff, 0x0000ffff}; return c[(x / 5 + y) % 3]; }
    if (band < 9) return rgba(noise % 256, noise / 256 % 256, noise / 7 % 256, 255);
    return rgba((120 + x) % 256, 60, (30 + y) % 256, x * 4 % 256);
}

static long encode(const uint32_t *px, long n, uint8_t *out) {
    long o = 0;
    uint8_t hdr[14] = {'q', 'o', 'i', 'f', 0, 0, W >> 8, W & 255, 0, 0, H >> 8, H & 255, 4, 0};
    memcpy(out, hdr, 14);
    o = 14;
    uint32_t seen[64] = {0}, prev = rgba(0, 0, 0, 255);
    long run = 0;
    for (long i = 0; i < n; i++) {
        uint32_t p = px[i];
        if (p == prev) {
            run++;
            if (run == 62 || i == n - 1) { out[o++] = 192 | (run - 1); run = 0; }
            continue;
        }
        if (run > 0) { out[o++] = 192 | (run - 1); run = 0; }
        uint32_t h = slot(p);
        if (seen[h] == p) {
            out[o++] = h;
        } else {
            seen[h] = p;
            if ((p & 255) == (prev & 255)) {
                int dr = (int8_t)((p >> 24) - (prev >> 24)), dg = (int8_t)((p >> 16 & 255) - (prev >> 16 & 255)), db = (int8_t)((p >> 8 & 255) - (prev >> 8 & 255));
                int dr_dg = dr - dg, db_dg = db - dg;
                if (dr > -3 && dr < 2 && dg > -3 && dg < 2 && db > -3 && db < 2) out[o++] = 64 | (dr + 2) << 4 | (dg + 2) << 2 | (db + 2);
                else if (dr_dg > -9 && dr_dg < 8 && dg > -33 && dg < 32 && db_dg > -9 && db_dg < 8) { out[o++] = 128 | (dg + 32); out[o++] = (dr_dg + 8) << 4 | (db_dg + 8); }
                else { out[o++] = 254; out[o++] = p >> 24; out[o++] = p >> 16 & 255; out[o++] = p >> 8 & 255; }
            } else {
                out[o++] = 255; out[o++] = p >> 24; out[o++] = p >> 16 & 255; out[o++] = p >> 8 & 255; out[o++] = p & 255;
            }
        }
        prev = p;
    }
    for (int k = 0; k < 7; k++) out[o++] = 0;
    out[o++] = 1;
    return o;
}

static void decode(const uint8_t *d, long n, uint32_t *px) {
    uint32_t seen[64] = {0}, prev = rgba(0, 0, 0, 255);
    long pos = 14, i = 0;
    while (i < n) {
        uint8_t b = d[pos];
        uint32_t p;
        if (b == 254) { p = rgba(d[pos + 1], d[pos + 2], d[pos + 3], prev & 255); pos += 4; }
        else if (b == 255) { p = rgba(d[pos + 1], d[pos + 2], d[pos + 3], d[pos + 4]); pos += 5; }
        else if (b >> 6 == 0) { p = seen[b]; pos += 1; }
        else if (b >> 6 == 1) { p = rgba(((prev >> 24) + (b >> 4 & 3) - 2) & 255, ((prev >> 16 & 255) + (b >> 2 & 3) - 2) & 255, ((prev >> 8 & 255) + (b & 3) - 2) & 255, prev & 255); pos += 1; }
        else if (b >> 6 == 2) { int dg = (b & 63) - 32; uint8_t m = d[pos + 1]; p = rgba(((prev >> 24) + dg + (m >> 4) - 8) & 255, ((prev >> 16 & 255) + dg) & 255, ((prev >> 8 & 255) + dg + (m & 15) - 8) & 255, prev & 255); pos += 2; }
        else { long r = (b & 63) + 1; for (long k = 0; k < r; k++) px[i++] = prev; pos += 1; continue; }
        seen[slot(p)] = p;
        px[i++] = p;
        prev = p;
    }
}

int main(void) {
    long n = (long)W * H;
    uint32_t *px = malloc(n * 4), *back = malloc(n * 4);
    uint8_t *data = malloc(n * 5 + 22);
    for (long i = 0; i < n; i++) px[i] = pixel(i);
    long size = 0, ok = 1;
    for (int rep = 0; rep < 5; rep++) {
        size = encode(px, n, data);
        decode(data, n, back);
        ok = ok && memcmp(px, back, n * 4) == 0;
    }
    long long check = 0;
    for (long i = 0; i < size; i++) check += (long long)data[i] * (i % 251 + 1);
    printf("%ld bytes\n%lld\n%s\n", size, check, ok ? "true" : "false");
    return 0;
}
