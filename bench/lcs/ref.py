# The reference for lcs (Python with numpy): the longest common subsequence
# of two sequences of 20,000 letters from a 4-letter alphabet,
# a[i] = mix(i) mod 4 and b[j] = mix(1,000,000 + j) mod 4 (splitmix64).
# Row by row: row[0] = 0, and row[j] = prev[j-1] + 1 where a[i-1] equals
# b[j-1], else the larger of prev[j] and row[j-1] (equal to the running
# maximum of t[j] = prev[j-1] + 1 or prev[j], since rows never go down).
# Prints the length and the sum of the last row.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return ((x ^ x >> np.uint64(31)) >> np.uint64(1)).astype(np.int64)


n = m = 20000
with np.errstate(over="ignore"):
    a = mix(np.arange(n, dtype=np.uint64)) % 4
    b = mix(np.uint64(1000000) + np.arange(m, dtype=np.uint64)) % 4
row = np.zeros(m + 1, dtype=np.int64)
for i in range(n):
    t = row.copy()
    t[1:] = np.where(b == a[i], row[:-1] + 1, row[1:])
    row = np.maximum.accumulate(t)
print(row[m], int(row.sum()))
