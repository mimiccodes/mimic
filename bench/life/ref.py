# Conway's Game of Life on a wrapping 1,024 × 1,024 board, 150 generations
# from a splitmix64 start (numpy): the live cells, and those of row 0.
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

n = 1024
with np.errstate(over="ignore"):
    g = (mix(np.arange(n * n, dtype=np.uint64)) % np.uint64(3) == 0).astype(np.int64).reshape(n, n)
for _ in range(150):
    around = sum(np.roll(np.roll(g, dr, 0), dc, 1) for dr in (-1, 0, 1) for dc in (-1, 0, 1) if dr or dc)
    g = ((around == 3) | ((around == 2) & (g == 1))).astype(np.int64)
print(int(g.sum()), int(g[0].sum()))
