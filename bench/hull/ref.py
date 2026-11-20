# The reference for hull (pure Python): the convex hull of 2,000,000 points
# with integer coordinates in a disc of radius 10^6 (point k from
# splitmix64: x = mix(2k) mod (2*10^6 + 1) - 10^6, y likewise from
# mix(2k + 1), kept when inside the disc, else moved halfway to the
# centre), by Andrew's monotone chain: sorted by x then y, the lower and
# the upper chain, collinear points dropped. Prints the number of hull
# points, twice the hull's area, and the sum of the hull's x coordinates.
M = (1 << 64) - 1
R = 10**6


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def point(k):
    x = mix(2 * k) % (2 * R + 1) - R
    y = mix(2 * k + 1) % (2 * R + 1) - R
    return (x, y) if x * x + y * y <= R * R else (x // 2, y // 2)


def cross(o, a, b):
    return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])


ps = sorted(point(k) for k in range(2000000))
lower, upper = [], []
for p in ps:
    while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
        lower.pop()
    lower.append(p)
for p in reversed(ps):
    while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
        upper.pop()
    upper.append(p)
hull = lower[:-1] + upper[:-1]
area2 = sum(hull[i][0] * hull[(i + 1) % len(hull)][1] - hull[(i + 1) % len(hull)][0] * hull[i][1] for i in range(len(hull)))
print(len(hull), area2, sum(p[0] for p in hull))
