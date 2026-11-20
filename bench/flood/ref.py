# The reference for flood (pure Python with a deque): a breadth-first flood
# fill on a 2,000 × 2,000 grid where cell i is a wall when mix(i) mod 100
# is below 30 (splitmix64), from the first open cell at or after the
# centre (row 1,000, column 1,000, in reading order), moving up, down, left
# and right. Prints how many cells are reached, the sum of their distances
# from the start, and the largest distance.
from collections import deque

M = (1 << 64) - 1
W = 2000


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


wall = bytearray(1 if mix(i) % 100 < 30 else 0 for i in range(W * W))
start = W * 1000 + 1000
while wall[start]:
    start += 1
dist = [-1] * (W * W)
dist[start] = 0
q = deque([start])
while q:
    c = q.popleft()
    x, y = c % W, c // W
    for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
        if 0 <= nx < W and 0 <= ny < W:
            nc = ny * W + nx
            if not wall[nc] and dist[nc] < 0:
                dist[nc] = dist[c] + 1
                q.append(nc)
reached = [d for d in dist if d >= 0]
print(len(reached), sum(reached), max(reached))
