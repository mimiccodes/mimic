# The reference for pip (Python with numpy): 1,000,000 points tested
# against a polygon of 500 vertices by ray casting (the even-odd rule),
# in exact integer arithmetic. The polygon goes round a square of side
# 2*10^6: side s (bottom, right, top, left) has vertices at
# t = -10^6 + 16,000j (j below 125), each moved inward by
# mix(500 + 125s + j) mod 400,000 (splitmix64). Point k is
# (mix(2k) mod 2,200,001 - 1,100,000, mix(2k + 1) mod 2,200,001 - 1,100,000).
# An edge (a, b) counts for p when a.y > p.y differs from b.y > p.y and p
# lies left of it: with d = b.y - a.y and t = (p.x - a.x) d - (p.y - a.y)(b.x - a.x),
# t < 0 when d > 0, t > 0 when d < 0. Prints how many points are inside,
# and the sum of their x coordinates.
import numpy as np

M = (1 << 64) - 1
R = 10**6


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


poly = []
for s in range(4):
    for j in range(125):
        t, d = -R + 16000 * j, mix(500 + 125 * s + j) % 400000
        poly.append([(t, -R + d), (R - d, t), (-t, R - d), (-R + d, -t)][s])
px = np.array([mix(2 * k) % 2200001 - 1100000 for k in range(1000000)], dtype=np.int64)
py = np.array([mix(2 * k + 1) % 2200001 - 1100000 for k in range(1000000)], dtype=np.int64)
inside = np.zeros(len(px), dtype=bool)
for i in range(len(poly)):
    (ax, ay), (bx, by) = poly[i], poly[(i + 1) % len(poly)]
    d = by - ay
    t = (px - ax) * d - (py - ay) * (bx - ax)
    crosses = (ay > py) != (by > py)
    left = (t < 0) if d > 0 else (t > 0)
    inside ^= crosses & left
print(int(inside.sum()), int(px[inside].sum()))
