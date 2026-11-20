# The reference for rule110 (Python with numpy): the elementary cellular
# automaton Rule 110 on a ring of 500,000 cells, cell i starting alive when
# mix(i) mod 3 is 0 (splitmix64), for 1,000 generations. Prints the
# number of live cells at the end and the sum of their positions.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


n = 500000
with np.errstate(over="ignore"):
    c = (mix(np.arange(n, dtype=np.uint64)) % np.uint64(3) == 0).astype(np.int64)
for _ in range(1000):
    k = np.roll(c, 1) * 4 + c * 2 + np.roll(c, -1)
    c = (110 >> k) & 1
print(int(c.sum()), int(np.nonzero(c)[0].sum()))
