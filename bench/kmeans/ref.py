# 1,000,000 splitmix64 points in 16 clusters by Lloyd's algorithm, 25
# rounds, in whole numbers (numpy): the sum of the centres' coordinates, the
# first cluster's points and the total squared distance to the nearest
# centre. argmin picks the first centre on a tie, as the others do.
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

n, k = 1_000_000, 16
with np.errstate(over="ignore"):
    i = np.arange(n, dtype=np.uint64)
    xs = (mix(np.uint64(2) * i) % np.uint64(100001)).astype(np.int64)
    ys = (mix(np.uint64(2) * i + np.uint64(1)) % np.uint64(100001)).astype(np.int64)
cx, cy = xs[:k].copy(), ys[:k].copy()

def near():
    d = (cx[None, :] - xs[:, None]) ** 2 + (cy[None, :] - ys[:, None]) ** 2
    a = d.argmin(axis=1)
    return a, d[np.arange(n), a]

for _ in range(25):
    a, _ = near()
    count = np.bincount(a, minlength=k)
    sx, sy = np.bincount(a, xs, k).astype(np.int64), np.bincount(a, ys, k).astype(np.int64)
    has = count > 0
    cx[has], cy[has] = sx[has] // count[has], sy[has] // count[has]
a, d = near()
print(int(cx.sum() + cy.sum()), int((a == 0).sum()), int(d.sum()))
