# The reference for dct (Python with numpy): the 8 × 8 two-dimensional DCT
# (as JPEG uses it) of every block of ten 1920 × 1080 8-bit frames, in fixed
# point: the cosine table C[u][x] = round(4096 c(u) cos((2x + 1) u π / 16))
# (fixed below), the rows first (R = B C^T, B the block less 128), then the
# columns (Y = C R), then (Y + 2^23) >> 24 (floored). Frame f's pixel i:
# (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256 with noise from a
# linear congruence, as in bench/filters. Prints the sum of the absolute
# values of the coefficients and a checksum that weighs each by (i mod
# 251 + 1), coefficient i of a frame being at the place of its pixel.
import numpy as np

C = np.array([int(t) for t in "1448 1448 1448 1448 1448 1448 1448 1448 2009 1703 1138 400 -400 -1138 -1703 -2009 1892 784 -784 -1892 -1892 -784 784 1892 1703 -400 -2009 -1138 1138 2009 400 -1703 1448 -1448 -1448 1448 1448 -1448 -1448 1448 1138 -2009 400 1703 -1703 -400 2009 -1138 784 -1892 1892 -784 -784 1892 -1892 784 400 -1138 1703 -2009 2009 -1703 1138 -400".split()], dtype=np.int64).reshape(8, 8)
W, H = 1920, 1080
i = np.arange(W * H, dtype=np.int64)
x, y = i % W, i // W
weight = i % 251 + 1
total = check = 0
for f in range(10):
    noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 // 65536 % 32
    img = ((x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256 - 128).reshape(H // 8, 8, W // 8, 8).transpose(0, 2, 1, 3)
    r = img @ C.T
    out = (np.einsum("vy,abyu->abvu", C, r) + (1 << 23)) >> 24
    flat = out.transpose(0, 2, 1, 3).reshape(-1)
    total += int(np.abs(flat).sum())
    check += int((flat * weight).sum())
print(total, check)
