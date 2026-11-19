# The reference for varint (pure Python): 4,000,000 readings of a counter
# that mostly moves a little (k-th step: mix(k) mod 2001 - 1000) and every
# 97th step jumps (mix(k) mod 10^9, splitmix64), starting from 10^6. Each
# value is stored as its difference from the one before (the first from 0),
# zigzag-mapped to an unsigned number ((d << 1) ^ (d >> 63)) and written as
# LEB128 (7 bits a byte, low bits first, the high bit set on every byte but
# the last). Then decoded again. Prints the number of bytes, a checksum that
# weighs every byte by (i mod 251 + 1), and 1 when decoding gives the
# values back.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 4000000
vals, v = [], 1000000
for k in range(n):
    v += mix(k) % 1000000000 if k % 97 == 0 else mix(k) % 2001 - 1000
    vals.append(v)
out = bytearray()
prev = 0
for x in vals:
    d = x - prev
    prev = x
    z = (d << 1) ^ (d >> 63) if d < 0 else d << 1
    while z >= 128:
        out.append(z & 127 | 128)
        z >>= 7
    out.append(z)
check = sum(b * (i % 251 + 1) for i, b in enumerate(out))
back, prev, z, shift = [], 0, 0, 0
for b in out:
    z |= (b & 127) << shift
    shift += 7
    if b < 128:
        d = (z >> 1) ^ -(z & 1)
        prev += d
        back.append(prev)
        z = shift = 0
print(len(out), check, 1 if back == vals else 0)
