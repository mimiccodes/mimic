# The reference for floyd (Python with numpy): the shortest paths between
# all pairs of 800 nodes by Floyd-Warshall; each node has edges to 8 others
# (from splitmix64, weights 1 to 1,000; of two edges to one node the lighter
# counts). Prints the sum of the distances that exist, how many pairs have
# no path, and the longest shortest path.
import numpy as np

M = (1 << 64) - 1
FAR = 1000000000000


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 800
d = np.full((n, n), FAR, dtype=np.int64)
for i in range(n):
    for e in range(8):
        j = mix(8 * i + e) % n
        w = 1 + mix(1000000 + 8 * i + e) % 1000
        if j != i and w < d[i, j]:
            d[i, j] = w
np.fill_diagonal(d, 0)
for k in range(n):
    d = np.minimum(d, d[:, k:k + 1] + d[k:k + 1, :])
some = d[d < FAR]
print(int(some.sum()), n * n - some.size, int(some.max()))
