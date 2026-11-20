# The reference for bsearch (Python with numpy): 1,000,000 sorted keys
# (mix(k) mod 10^12 for k below 1,000,000, splitmix64, then sorted) and
# 4,000,000 lookups of q = mix(10^9 + j) mod 10^12: for each, the first
# position whose key is not below q (the lower bound; 1,000,000 when every
# key is below). Prints the sum of the positions and how many lookups found
# their key exactly (every 16th lookup asks for a key that is there:
# key[(j div 16 * 7919) mod 1,000,000] when j mod 16 is 0).
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return ((x ^ x >> np.uint64(31)) >> np.uint64(1)).astype(np.int64)


n, m = 1000000, 4000000
with np.errstate(over="ignore"):
    keys = np.sort(mix(np.arange(n, dtype=np.uint64)) % 10**12)
    j = np.arange(m, dtype=np.int64)
    q = mix(np.uint64(10**9) + j.astype(np.uint64)) % 10**12
hit = j % 16 == 0
q[hit] = keys[(j[hit] // 16 * 7919) % n]
pos = np.searchsorted(keys, q, side="left")
found = int(((pos < n) & (keys[np.minimum(pos, n - 1)] == q)).sum())
print(int(pos.sum()), found)
