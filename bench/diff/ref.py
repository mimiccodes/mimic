# The reference for diff: 1,000 pairs of "files" (1,000 to 2,000 lines,
# each line one of 50 texts, from splitmix64); the second of each pair is
# the first with about 3% of lines deleted, 3% changed and 3% added. The
# shortest edit script's length (lines deleted plus added) by Myers'
# algorithm, checked on the first pairs against the textbook longest
# common subsequence (n + m - 2 LCS). Prints the sum and the largest.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def files(p):
    a = [mix(p * 100000 + i) % 50 for i in range(1000 + mix(p * 10) % 1001)]
    b = []
    for i, line in enumerate(a):
        r = mix(p * 100000 + 50000 + i) % 100
        if r < 3:
            continue
        if r < 6:
            b.append(mix(p * 100000 + 70000 + i) % 50)
        elif r < 9:
            b += [mix(p * 100000 + 90000 + i) % 50, line]
        else:
            b.append(line)
    return a, b


def myers(a, b):
    n, m = len(a), len(b)
    off = n + m + 1
    v = [0] * (2 * off + 1)
    for d in range(n + m + 1):
        for k in range(-d, d + 1, 2):
            x = v[off + k + 1] if k == -d or (k != d and v[off + k - 1] < v[off + k + 1]) else v[off + k - 1] + 1
            y = x - k
            while x < n and y < m and a[x] == b[y]:
                x, y = x + 1, y + 1
            v[off + k] = x
            if x >= n and y >= m:
                return d


def lcs(a, b):
    prev = [0] * (len(b) + 1)
    for x in a:
        cur = [0]
        for j, y in enumerate(b):
            cur.append(prev[j] + 1 if x == y else max(prev[j + 1], cur[j]))
        prev = cur
    return prev[-1]


d = []
for p in range(1000):
    a, b = files(p)
    d.append(myers(a, b))
    if p < 3:
        assert d[-1] == len(a) + len(b) - 2 * lcs(a, b)
print(sum(d), max(d))
