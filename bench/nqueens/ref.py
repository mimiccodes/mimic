# The reference for nqueens (Python): the number of ways to put 15 queens
# on a 15x15 board so that none attacks another (backtracking with bit
# masks). Prints the count.
import sys

sys.setrecursionlimit(100)


def solve(n, cols, left, right):
    full = (1 << n) - 1
    if cols == full:
        return 1
    count = 0
    free = full & ~(cols | left | right)
    while free:
        bit = free & -free
        free ^= bit
        count += solve(n, cols | bit, (left | bit) << 1 & full, (right | bit) >> 1)
    return count


print(solve(15, 0, 0, 0))
