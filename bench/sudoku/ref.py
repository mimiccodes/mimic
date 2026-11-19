# The reference for sudoku (pure Python): 200 puzzles, each a full grid
# (cell (r, c) holds 1 + (3(r mod 3) + r div 3 + c) mod 9, with the digits
# relabelled by a permutation from splitmix64) with the cells where
# mix(81p + i) mod 100 < 62 emptied. Each is solved by backtracking: the
# first empty cell in reading order gets the digits 1 … 9 that fit, in
# order, the first complete grid wins. Prints the sum over the puzzles of
# the top row of the solution read as a 9-digit number, and the number of
# cells tried in all (each placement attempted).
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def puzzle(p):
    perm = list(range(1, 10))
    for i in range(8, 0, -1):
        j = mix(1000 * p + i) % (i + 1)
        perm[i], perm[j] = perm[j], perm[i]
    g = [perm[(3 * (r % 3) + r // 3 + c) % 9] for r in range(9) for c in range(9)]
    return [0 if mix(81 * p + i) % 100 < 62 else g[i] for i in range(81)]


def fits(b, i, v):
    r, c = divmod(i, 9)
    for k in range(9):
        if b[r * 9 + k] == v or b[k * 9 + c] == v:
            return False
    br, bc = r - r % 3, c - c % 3
    for dr in range(3):
        for dc in range(3):
            if b[(br + dr) * 9 + bc + dc] == v:
                return False
    return True


tried = 0


def solve(b):
    global tried
    try:
        i = b.index(0)
    except ValueError:
        return True
    for v in range(1, 10):
        tried += 1
        if fits(b, i, v):
            b[i] = v
            if solve(b):
                return True
            b[i] = 0
    return False


total = 0
for p in range(200):
    b = puzzle(p)
    assert solve(b)
    total += int("".join(map(str, b[:9])))
print(total, tried)
