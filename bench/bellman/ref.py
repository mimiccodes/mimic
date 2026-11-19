# The reference for bellman (Python with numpy): single-source shortest
# paths from node 0 in a random directed graph of 50,000 nodes and 400,000
# edges with weights 1 … 1,000 (edge e goes from mix(3e) mod n to
# mix(3e + 1) mod n with weight 1 + mix(3e + 2) mod 1,000; splitmix64),
# relaxed round after round until nothing changes (Bellman-Ford). Prints
# the number of nodes reached, the sum of their distances, and the
# largest distance.
import numpy as np

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 50000, 400000
src = np.array([mix(3 * e) % n for e in range(m)])
dst = np.array([mix(3 * e + 1) % n for e in range(m)])
w = np.array([1 + mix(3 * e + 2) % 1000 for e in range(m)], dtype=np.int64)
INF = 1 << 60
d = np.full(n, INF, dtype=np.int64)
d[0] = 0
while True:
    cand = np.where(d[src] < INF, d[src] + w, INF)
    nd = d.copy()
    np.minimum.at(nd, dst, cand)
    if (nd == d).all():
        break
    d = nd
reached = d < INF
print(int(reached.sum()), int(d[reached].sum()), int(d[reached].max()))
