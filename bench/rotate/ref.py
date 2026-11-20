# The reference for rotate (Python with numpy): six 3840×2160 8-bit frames,
# each turned 90° clockwise four times (3840×2160 → 2160×3840 → …); after
# each turn, a checksum that weighs every pixel by (i mod 251 + 1) in the
# turned image's order. Prints the frame and its four checksums.
import numpy as np

W, H = 3840, 2160
i = np.arange(W * H, dtype=np.int64)
x, y = i % W, i // W
weight = i % 251 + 1
for f in range(6):
    noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 // 65536 % 32
    img = ((x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256).reshape(H, W)
    sums = []
    for _ in range(4):
        img = np.rot90(img, -1)
        sums.append(int((img.reshape(-1) * weight).sum()))
    print(f, *sums)
