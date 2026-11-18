// utf8: 20,000 generated documents encoded as UTF-8 and validated by a
// strict state machine (RFC 3629), one byte at a time; every fifth has a
// byte replaced by 0xFF, the one after loses its last byte. Prints the
// valid documents, their characters, and all the bytes checked.
#include <stdint.h>
#include <stdio.h>

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t ch(int64_t r) {
    int64_t kind = r % 10, v = r >> 8;
    if (kind < 6) return 0x20 + v % 95;
    if (kind < 8) return 0xA0 + v % (0x800 - 0xA0);
    if (kind < 9) {
        int64_t c = 0x800 + v % (0x10000 - 0x800);
        return c >= 0xD800 && c <= 0xDFFF ? c + 0x800 : c;
    }
    return 0x10000 + v % 0x100000;
}

static int encode(int64_t c, uint8_t *o) {
    if (c < 0x80) { o[0] = (uint8_t)c; return 1; }
    if (c < 0x800) { o[0] = (uint8_t)(0xC0 | c >> 6); o[1] = (uint8_t)(0x80 | (c & 63)); return 2; }
    if (c < 0x10000) { o[0] = (uint8_t)(0xE0 | c >> 12); o[1] = (uint8_t)(0x80 | (c >> 6 & 63)); o[2] = (uint8_t)(0x80 | (c & 63)); return 3; }
    o[0] = (uint8_t)(0xF0 | c >> 18); o[1] = (uint8_t)(0x80 | (c >> 12 & 63)); o[2] = (uint8_t)(0x80 | (c >> 6 & 63)); o[3] = (uint8_t)(0x80 | (c & 63));
    return 4;
}

int main(void) {
    static uint8_t b[1300];
    int64_t valid = 0, chars = 0, total = 0;
    for (int64_t d = 0; d < 20000; d++) {
        int64_t n = 50 + mix(d * 7777) % 251, len = 0;
        for (int64_t i = 0; i < n; i++) len += encode(ch(mix(d * 100000 + i)), b + len);
        if (d % 5 == 0) b[mix(d * 31) % len] = 0xFF;
        else if (d % 5 == 1) len--;
        total += len;
        int need = 0, ok = 1;
        int64_t c = 0;
        uint8_t lo = 0x80, hi = 0xBF;
        for (int64_t i = 0; i < len && ok; i++) {
            uint8_t x = b[i];
            if (need > 0) {
                if (x < lo || x > hi) ok = 0;
                else { if (--need == 0) c++; lo = 0x80; hi = 0xBF; }
            } else if (x < 0x80) c++;
            else if (x >= 0xC2 && x <= 0xDF) { need = 1; lo = 0x80; hi = 0xBF; }
            else if (x == 0xE0) { need = 2; lo = 0xA0; hi = 0xBF; }
            else if (x == 0xED) { need = 2; lo = 0x80; hi = 0x9F; }
            else if (x >= 0xE1 && x <= 0xEF) { need = 2; lo = 0x80; hi = 0xBF; }
            else if (x == 0xF0) { need = 3; lo = 0x90; hi = 0xBF; }
            else if (x >= 0xF1 && x <= 0xF3) { need = 3; lo = 0x80; hi = 0xBF; }
            else if (x == 0xF4) { need = 3; lo = 0x80; hi = 0x8F; }
            else ok = 0;
        }
        if (ok && need == 0) { valid++; chars += c; }
    }
    printf("%lld %lld %lld\n", (long long)valid, (long long)chars, (long long)total);
    return 0;
}
