# The reference for segments (Python with numpy): 8,000 segments on a
# 100,000 × 100,000 grid (segment k starts at (mix(4k), mix(4k + 1)) mod
# 100,000 and ends up to 2,000 away: + mix(4k + 2), mix(4k + 3) mod 4,001
# - 2,000; splitmix64), and every pair tested for meeting by orientations:
# they meet when each one's ends lie on different sides of the other, or
# when an end lies on the other segment (collinear touching counts).
# Prints the number of pairs that meet, and the sum of i + j over them.
import numpy as np

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 8000
ax = np.array([mix(4 * k) % 100000 for k in range(n)], dtype=np.int64)
ay = np.array([mix(4 * k + 1) % 100000 for k in range(n)], dtype=np.int64)
bx = ax + np.array([mix(4 * k + 2) % 4001 - 2000 for k in range(n)], dtype=np.int64)
by = ay + np.array([mix(4 * k + 3) % 4001 - 2000 for k in range(n)], dtype=np.int64)


def orient(px, py, qx, qy, rx, ry):
    return np.sign((qx - px) * (ry - py) - (qy - py) * (rx - px))


def on(px, py, qx, qy, rx, ry):
    # r on the segment p-q, given the three are collinear
    return (np.minimum(px, qx) <= rx) & (rx <= np.maximum(px, qx)) & (np.minimum(py, qy) <= ry) & (ry <= np.maximum(py, qy))


count = total = 0
for i in range(n - 1):
    j = np.arange(i + 1, n)
    o1 = orient(ax[i], ay[i], bx[i], by[i], ax[j], ay[j])
    o2 = orient(ax[i], ay[i], bx[i], by[i], bx[j], by[j])
    o3 = orient(ax[j], ay[j], bx[j], by[j], ax[i], ay[i])
    o4 = orient(ax[j], ay[j], bx[j], by[j], bx[i], by[i])
    meet = ((o1 != o2) & (o3 != o4)) \
        | ((o1 == 0) & on(ax[i], ay[i], bx[i], by[i], ax[j], ay[j])) \
        | ((o2 == 0) & on(ax[i], ay[i], bx[i], by[i], bx[j], by[j])) \
        | ((o3 == 0) & on(ax[j], ay[j], bx[j], by[j], ax[i], ay[i])) \
        | ((o4 == 0) & on(ax[j], ay[j], bx[j], by[j], bx[i], by[i]))
    count += int(meet.sum())
    total += int((i + j[meet]).sum())
print(count, total)
