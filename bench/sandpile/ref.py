# The reference for sandpile (Python with numpy): an abelian sandpile on a
# 128 × 128 grid, cell i starting with 4 + mix(i) mod 30 grains (splitmix64).
# In each round every cell at once sends s div 4 grains to each of its four
# neighbours and keeps s mod 4 (grains sent off the edge are lost), until
# no cell has 4 or more. Prints the number of rounds, the grains left, and
# how many cells end with 0, 1, 2 and 3.
import numpy as np

L = 128


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


with np.errstate(over="ignore"):
    s = (4 + mix(np.arange(L * L, dtype=np.uint64)) % np.uint64(30)).astype(np.int64).reshape(L, L)
rounds = 0
while (s >= 4).any():
    g = s // 4
    s = s % 4
    s[1:, :] += g[:-1, :]
    s[:-1, :] += g[1:, :]
    s[:, 1:] += g[:, :-1]
    s[:, :-1] += g[:, 1:]
    rounds += 1
print(rounds, int(s.sum()), *[int((s == v).sum()) for v in range(4)])
