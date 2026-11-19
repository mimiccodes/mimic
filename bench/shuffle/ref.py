# The reference for shuffle (Python with numpy for the checksum): the
# numbers 0 … 9,999,999 shuffled by Fisher-Yates, i from the top down to 1
# swapping place i with place mix(i) mod (i + 1) (splitmix64). Prints the
# first and the last number and the sum of each number times
# (i mod 251 + 1).
import numpy as np

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 10000000
a = list(range(n))
for i in range(n - 1, 0, -1):
    j = mix(i) % (i + 1)
    a[i], a[j] = a[j], a[i]
v = np.array(a, dtype=np.int64)
print(a[0], a[-1], int((v * (np.arange(n) % 251 + 1)).sum()))
