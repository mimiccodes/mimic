# The reference for histeq (Python with numpy): six 3840×2160 8-bit frames
# of low contrast (64 … 191), each equalized: a histogram of 256 bins, its
# running sum (cdf), the smallest nonzero cdf value m, and every pixel v
# mapped to ((cdf[v] - m) * 255 + (n - m) div 2) div (n - m), at least 0.
# Prints, per frame, the sum of the result and a checksum that weighs every
# pixel by (i mod 251 + 1).
import numpy as np

W, H = 3840, 2160
n = W * H
i = np.arange(n, dtype=np.int64)
x, y = i % W, i // W
weight = i % 251 + 1
for f in range(6):
    noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 // 65536 % 32
    img = 64 + (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 128
    hist = np.bincount(img, minlength=256)
    cdf = np.cumsum(hist)
    m = cdf[hist > 0][0]
    lut = np.maximum(0, ((cdf - m) * 255 + (n - m) // 2) // (n - m))
    out = lut[img]
    print(f, int(out.sum()), int((out * weight).sum()))
