# The reference for select (Python with numpy): 20,000,000 values
# mix(k) mod 10^9 (splitmix64) and their k-th smallest for k = 1%, 10%,
# 50%, 90% and 99% of the way (k = n * p div 100, counting from 0). The
# benchmark finds each by quickselect; any correct method gives the same
# values. Prints the five values.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return ((x ^ x >> np.uint64(31)) >> np.uint64(1)).astype(np.int64)


n = 20000000
with np.errstate(over="ignore"):
    xs = mix(np.arange(n, dtype=np.uint64)) % 10**9
ks = [n * p // 100 for p in (1, 10, 50, 90, 99)]
print(*np.partition(xs, ks)[ks])
