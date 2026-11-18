# The reference for pagerank (Python): 20 iterations of PageRank (damping
# 0.85; rank from nodes without outgoing edges is not passed on) on the
# bfs random graph at half the size, each node's new rank summed over the
# edges into it in order of their source. Prints the sum of the ranks and the
# largest rank times n, to 6 decimals.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 100000, 500000
src = [mix(2 * e + 1) % n for e in range(m)]
dst = [mix(2 * e + 2) % n for e in range(m)]

outdeg = [0] * n
into = [[] for _ in range(n)]
for e in range(m):
    outdeg[src[e]] += 1
    into[dst[e]].append(src[e])
for lst in into:
    lst.sort()
pr = [1.0 / n] * n
for _ in range(20):
    contrib = [pr[u] / outdeg[u] if outdeg[u] else 0.0 for u in range(n)]
    pr = [0.15 / n + 0.85 * sum(contrib[u] for u in into[v]) for v in range(n)]
print(f"{sum(pr):.6f} {max(pr) * n:.6f}")
