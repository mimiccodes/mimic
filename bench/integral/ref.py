# The reference for integral (Python with numpy): a 2048×2048 8-bit image,
# its summed-area table (with a row and a column of zeros in front), and
# 8,000,000 box sums from it: for query q, x1 = mix(4q) % 2048,
# x2 = mix(4q + 1) % 2048, y1 = mix(4q + 2) % 2048, y2 = mix(4q + 3) % 2048
# (splitmix64), each pair put in order, the box taking both ends. Prints the
# total of the box sums and the image's sum.
import numpy as np

W = H = 2048
M = np.uint64((1 << 64) - 1)


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return ((x ^ x >> np.uint64(31)) >> np.uint64(1)).astype(np.int64)


i = np.arange(W * H, dtype=np.int64)
x, y = i % W, i // W
noise = (i * 1103515245 + 12345) % 2147483648 // 65536 % 32
img = ((x * 7 + y * 13 + x * y % 31 + noise) % 256).reshape(H, W)
sat = np.zeros((H + 1, W + 1), dtype=np.int64)
sat[1:, 1:] = img.cumsum(0).cumsum(1)
total = 0
for start in range(0, 8000000, 1000000):
    q = np.arange(start, start + 1000000, dtype=np.uint64)
    with np.errstate(over="ignore"):
        a, b = mix(4 * q) % W, mix(4 * q + np.uint64(1)) % W
        c, d = mix(4 * q + np.uint64(2)) % H, mix(4 * q + np.uint64(3)) % H
    x1, x2 = np.minimum(a, b), np.maximum(a, b)
    y1, y2 = np.minimum(c, d), np.maximum(c, d)
    total += int((sat[y2 + 1, x2 + 1] - sat[y1, x2 + 1] - sat[y2 + 1, x1] + sat[y1, x1]).sum())
print(total, int(sat[H, W]))
