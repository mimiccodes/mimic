# Heat over a 1,024 × 1,024 plate for 400 steps (numpy): the sum of the
# cells and the centre, in thousandths.
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

n = 1024
with np.errstate(over="ignore"):
    g = (mix(np.arange(n * n, dtype=np.uint64)) % np.uint64(100000)).astype(np.float64).reshape(n, n) / 1000.0
for _ in range(400):
    h = g.copy()
    x = g[1:-1, 1:-1]
    h[1:-1, 1:-1] = (x + x + x + x + g[:-2, 1:-1] + g[2:, 1:-1] + g[1:-1, :-2] + g[1:-1, 2:]) * 0.125
    g = h
print(int((g * 1000.0).astype(np.int64).sum()), int(g[n // 2, n // 2] * 1000.0))
