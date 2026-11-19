# The reference for manacher (pure Python): Manacher's algorithm over
# 2,000,000 letters from a 2-letter alphabet (letter i: mix(i div 2) mod 2,
# splitmix64). The text t interleaves a separator with the letters
# (t[2k + 1] is letter k, t[2k] the separator, 2n + 1 places); p[i] is the
# largest radius with t[i - p … i + p] a palindrome, which is also the
# length of the longest palindrome of the letters centred there. Prints the
# number of palindromic substrings (the sum of (p[i] + 1) div 2), the
# longest palindrome's length, and where the first of that length starts.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 2000000
s = [mix(i // 2) % 2 for i in range(n)]
m = 2 * n + 1
t = [2] * m
t[1::2] = s
p = [0] * m
c = r = 0
for i in range(m):
    k = min(r - i, p[2 * c - i]) if i < r else 0
    while i - k - 1 >= 0 and i + k + 1 < m and t[i - k - 1] == t[i + k + 1]:
        k += 1
    p[i] = k
    if i + k > r:
        c, r = i, i + k
best = max(p)
at = p.index(best)
print(sum((x + 1) // 2 for x in p), best, (at - best) // 2)
