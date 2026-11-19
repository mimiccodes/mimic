# The reference for rangemin (Python with numpy): range-minimum queries
# over 1,000,000 values (mix(i) mod 10^9, splitmix64) with a sparse table:
# level k holds the minimum of each run of 2^k values. Query q asks for the
# smallest value in [a, b], a and b being mix(4,000,000 + 2q) and
# mix(4,000,000 + 2q + 1) mod 1,000,000 in order; there are 4,000,000
# queries. Prints the sum of the answers and the sum of the query lengths.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


n = 1000000
m = 4000000
with np.errstate(over="ignore"):
    xs = (mix(np.arange(n, dtype=np.uint64)) % np.uint64(10**9)).astype(np.int64)
    q = np.arange(m, dtype=np.uint64)
    u = (mix(np.uint64(m) + 2 * q) % np.uint64(n)).astype(np.int64)
    v = (mix(np.uint64(m) + 2 * q + np.uint64(1)) % np.uint64(n)).astype(np.int64)
a, b = np.minimum(u, v), np.maximum(u, v)
levels = [xs]
k = 1
while (1 << k) <= n:
    prev = levels[-1]
    h = 1 << (k - 1)
    levels.append(np.minimum(prev[:-h], prev[h:]))
    k += 1
length = b - a + 1
lg = np.floor(np.log2(length)).astype(np.int64)
ans = np.empty(m, dtype=np.int64)
for k in range(len(levels)):
    sel = lg == k
    if sel.any():
        lv = levels[k]
        ans[sel] = np.minimum(lv[a[sel]], lv[b[sel] - (1 << k) + 1])
print(int(ans.sum()), int(length.sum()))
