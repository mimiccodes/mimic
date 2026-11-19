# The reference for radix (Python with numpy): 8,000,000 32-bit keys,
# key k = mix(k) mod 2^32 (splitmix64), sorted. (The benchmark sorts them by
# LSD radix sort, four passes of 8 bits; any correct sort gives the same
# order.) Prints the smallest, the median and the largest, and the sum of
# each key times (i mod 251 + 1).
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


n = 8000000
with np.errstate(over="ignore"):
    keys = (mix(np.arange(n, dtype=np.uint64)) % np.uint64(1 << 32)).astype(np.int64)
s = np.sort(keys)
i = np.arange(n, dtype=np.int64)
print(s[0], s[n // 2], s[n - 1], int((s * (i % 251 + 1)).sum()))
