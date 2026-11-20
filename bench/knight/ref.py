# The reference for knight (pure Python; slow, several minutes): the number
# of knight's tours (paths visiting every square once, open or closed) on a
# 5 × 5 board from each square, by trying the eight moves in a fixed order
# with the visited squares as a bitmask, and the number of moves tried in
# all. The totals agree with the known counts: 1,728 tours, 304 from each
# corner. Prints the total, the number from the corner (0, 0), and the moves
# tried.
import sys

sys.setrecursionlimit(100)
N = 5
MOVES = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)]
tried = 0


def tours(x, y, mask, depth):
    global tried
    if depth == N * N:
        return 1
    t = 0
    for dx, dy in MOVES:
        nx, ny = x + dx, y + dy
        tried += 1
        if 0 <= nx < N and 0 <= ny < N and not mask >> (ny * N + nx) & 1:
            t += tours(nx, ny, mask | 1 << (ny * N + nx), depth + 1)
    return t


counts = [tours(s % N, s // N, 1 << s, 1) for s in range(N * N)]
print(sum(counts), counts[0], tried)
