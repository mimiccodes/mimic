# The reference for matchain (Python with numpy): the cheapest order to
# multiply a chain of 600 matrices (dimensions d[0] … d[600], d[i] = 10 +
# mix(i) mod 90, splitmix64) by the interval dynamic programme:
# cost[i][j] = min over k of cost[i][k] + cost[k + 1][j] + d[i] d[k + 1] d[j + 1].
# Prints the cheapest cost of the whole chain, and the sum of the cheapest
# costs of all sub-chains.
import numpy as np

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 600
d = np.array([10 + mix(i) % 90 for i in range(n + 1)], dtype=np.int64)
cost = np.zeros((n, n), dtype=np.int64)
for length in range(2, n + 1):
    for i in range(n - length + 1):
        j = i + length - 1
        k = np.arange(i, j)
        cost[i, j] = (cost[i, k] + cost[k + 1, j] + d[i] * d[k + 1] * d[j + 1]).min()
print(int(cost[0, n - 1]), int(np.triu(cost).sum()))
