# The reference for bloom (Python): a Bloom filter of 8,388,608 cells and
# 7 hashes per key (double hashing from splitmix64); keys 0 to 999,999 go
# in, then keys 0 to 1,999,999 are looked up. Prints how many lookups say
# "maybe" and how many cells are set.
M = (1 << 64) - 1
CELLS = 8388608


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def cells(x):
    h1, h2 = mix(2 * x), mix(2 * x + 1) | 1
    return [(h1 + j * h2) & M & (CELLS - 1) for j in range(7)]


n = 1000000
f = bytearray(CELLS)
for x in range(n):
    for c in cells(x):
        f[c] = 1
maybe = sum(all(f[c] for c in cells(x)) for x in range(2 * n))
print(maybe, sum(f))
