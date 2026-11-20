# The reference for gauss (pure Python): the determinant and the rank of a
# 500 × 500 matrix modulo the prime 1,000,000,007 (entry (i, j):
# mix(500i + j) mod p, splitmix64; row 499 is row 0 plus row 1, so the rank
# is 499 and the determinant 0), and then of the same matrix with row 499
# replaced by mix(10^6 + j) mod p. Gaussian elimination: for each column,
# the first row from the current one with a nonzero entry is swapped up
# (each swap negates the determinant), and the rows below have their
# entries in that column cleared. Prints, for each matrix, the rank and the
# determinant.
M = (1 << 64) - 1
P = 1000000007


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 500


def solve(a):
    a = [row[:] for row in a]
    det, rank = 1, 0
    for col in range(n):
        piv = next((r for r in range(rank, n) if a[r][col]), None)
        if piv is None:
            det = 0
            continue
        if piv != rank:
            a[rank], a[piv] = a[piv], a[rank]
            det = P - det if det else 0
        det = det * a[rank][col] % P
        inv = pow(a[rank][col], P - 2, P)
        for r in range(rank + 1, n):
            f = a[r][col] * inv % P
            if f:
                a[r] = [(x - f * y) % P for x, y in zip(a[r], a[rank])]
        rank += 1
    return rank, det


a = [[mix(n * i + j) % P for j in range(n)] for i in range(n)]
a[n - 1] = [(a[0][j] + a[1][j]) % P for j in range(n)]
r1, d1 = solve(a)
a[n - 1] = [mix(10**6 + j) % P for j in range(n)]
r2, d2 = solve(a)
print(r1, d1, r2, d2)
