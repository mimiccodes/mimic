# The reference for levenshtein (Python): the edit distance of 20,000
# pairs of generated DNA-like strings (30 to 60 letters of "acgt", each
# letter from splitmix64), by the usual dynamic program. Prints the sum of
# the distances and the largest.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def word(k):
    n = 30 + mix(k * 1000) % 31
    return [mix(k * 1000 + 1 + i) % 4 for i in range(n)]


def dist(a, b):
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


d = [dist(word(2 * p), word(2 * p + 1)) for p in range(20000)]
print(sum(d), max(d))
