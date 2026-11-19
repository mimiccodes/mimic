# The reference for tsp (Python with numpy): the shortest round trip through
# 20 cities by the Held-Karp dynamic programme. City c is at
# (mix(2c) mod 1000, mix(2c + 1) mod 1000) (splitmix64); distances are
# Manhattan. dp[S][j] is the shortest path from city 0 through the set S of
# other cities, ending at j in S. Prints the length of the shortest tour
# and the sum of dp over all full-length paths (S everything, each j).
import numpy as np

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 20
xs = [mix(2 * c) % 1000 for c in range(n)]
ys = [mix(2 * c + 1) % 1000 for c in range(n)]
d = np.array([[abs(xs[a] - xs[b]) + abs(ys[a] - ys[b]) for b in range(n)] for a in range(n)], dtype=np.int64)
m = n - 1  # cities 1 … n - 1 are bits 0 … m - 1
INF = 1 << 50
dp = np.full((1 << m, m), INF, dtype=np.int64)
for j in range(m):
    dp[1 << j, j] = d[0, j + 1]
masks = np.arange(1 << m)
pop = np.array([bin(s).count("1") for s in range(1 << m)])
dd = d[1:, 1:]  # dd[k, j]: from city k + 1 to city j + 1
for c in range(2, m + 1):
    S = masks[pop == c]
    for j in range(m):
        Sj = S[(S >> j) & 1 == 1]
        prev = Sj ^ (1 << j)
        cand = dp[prev] + dd[:, j][None, :]
        cand[((prev[:, None] >> np.arange(m)[None, :]) & 1) == 0] = INF
        dp[Sj, j] = cand.min(axis=1)
full = (1 << m) - 1
print(int((dp[full] + d[1:, 0]).min()), int(dp[full].sum()))
