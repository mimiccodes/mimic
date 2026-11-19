# The reference for maze (pure Python): a 500 × 500 maze carved by a
# depth-first search with a stack from cell 0: at each step, the cell on
# top of the stack looks at its unvisited neighbours (up, right, down, left
# in that order); if there are any, it opens the wall to the one numbered
# mix(step) mod count (splitmix64, step counting every look) and pushes it,
# else it is popped. Then a breadth-first search from the top-left cell
# finds the distance to every cell. Prints the distance to the bottom-right
# cell, the number of dead ends (cells with one opening), and the sum of
# all distances.
from collections import deque

M = (1 << 64) - 1
W = 500


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = W * W
opened = [0] * n  # bit 0 up, 1 right, 2 down, 3 left
seen = bytearray(n)
seen[0] = 1
stack, step = [0], 0
DIRS = [(-W, 0), (1, 1), (W, 2), (-1, 3)]
while stack:
    c = stack[-1]
    x, y = c % W, c // W
    free = []
    for d, (off, bit) in enumerate(DIRS):
        if d == 0 and y == 0 or d == 1 and x == W - 1 or d == 2 and y == W - 1 or d == 3 and x == 0:
            continue
        if not seen[c + off]:
            free.append(d)
    if free:
        d = free[mix(step) % len(free)]
        nxt = c + DIRS[d][0]
        opened[c] |= 1 << d
        opened[nxt] |= 1 << ((d + 2) % 4)
        seen[nxt] = 1
        stack.append(nxt)
    else:
        stack.pop()
    step += 1
dist = [-1] * n
dist[0] = 0
q = deque([0])
while q:
    c = q.popleft()
    for d, (off, bit) in enumerate(DIRS):
        if opened[c] >> d & 1 and dist[c + off] < 0:
            dist[c + off] = dist[c] + 1
            q.append(c + off)
print(dist[n - 1], sum(1 for o in opened if bin(o).count("1") == 1), sum(dist))
