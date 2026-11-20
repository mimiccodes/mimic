# The reference for randwalk (Python with numpy): 200,000 walkers, each
# taking 1,000 steps on the square lattice from (0, 0); step t of walker w
# goes east, north, west or south as mix(1000w + t) mod 4 is 0, 1, 2 or 3
# (splitmix64). Prints the sum of the squared distances at the end, the
# largest, and how many walkers end where they started.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


W, T = 200000, 1000
x = np.zeros(W, dtype=np.int64)
y = np.zeros(W, dtype=np.int64)
w = np.arange(W, dtype=np.uint64)
with np.errstate(over="ignore"):
    for t in range(T):
        d = (mix(w * np.uint64(1000) + np.uint64(t)) % np.uint64(4)).astype(np.int64)
        x += (d == 0).astype(np.int64) - (d == 2)
        y += (d == 1).astype(np.int64) - (d == 3)
r2 = x * x + y * y
print(int(r2.sum()), int(r2.max()), int(((x == 0) & (y == 0)).sum()))
