# The reference for utf8 (Python's own strict decoder): 20,000 generated
# documents of 50 to 300 characters (ASCII, 2-, 3- and 4-byte ones, from
# splitmix64), encoded as UTF-8; every fifth has one byte replaced by
# 0xFF, and the one after each of those loses its last byte. Prints the
# valid documents, their characters, and all the bytes checked.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def char(r):
    kind, v = r % 10, r >> 8
    if kind < 6:
        return 0x20 + v % 95
    if kind < 8:
        return 0xA0 + v % (0x800 - 0xA0)
    if kind < 9:
        c = 0x800 + v % (0x10000 - 0x800)
        return c + 0x800 if 0xD800 <= c <= 0xDFFF else c
    return 0x10000 + v % 0x100000


valid = chars = total = 0
for d in range(20000):
    n = 50 + mix(d * 7777) % 251
    b = bytearray("".join(chr(char(mix(d * 100000 + i))) for i in range(n)).encode())
    if d % 5 == 0:
        b[mix(d * 31) % len(b)] = 0xFF
    elif d % 5 == 1:
        b = b[:-1]
    total += len(b)
    try:
        s = bytes(b).decode("utf-8")
        valid += 1
        chars += len(s)
    except UnicodeDecodeError:
        pass
print(valid, chars, total)
