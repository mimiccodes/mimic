# π from 200,000,000 splitmix64 points (numpy, in chunks): the count inside
# the quarter circle, and 4 × count / points to six places.
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

n, count, step = 200_000_000, 0, 10_000_000
with np.errstate(over="ignore"):
    for lo in range(0, n, step):
        i = np.arange(lo, lo + step, dtype=np.uint64)
        x = (mix(2 * i) >> np.uint64(10)).astype(np.float64) / 9007199254740992.0
        y = (mix(2 * i + 1) >> np.uint64(10)).astype(np.float64) / 9007199254740992.0
        count += int(np.count_nonzero(x * x + y * y < 1.0))
micro = count * 4_000_000 // n
print(f"{count} {micro // 1_000_000}.{micro % 1_000_000:06d}")
