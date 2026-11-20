# The reference for zalgo (pure Python): the Z-array of 4,000,000 letters
# from a 4-letter alphabet (letter i: mix(i div 3) mod 4, splitmix64), by
# the Z-algorithm: z[i] is the length of the longest common prefix of the
# text and the text from i (z[0] = 0 here). Prints the sum of z, how many
# positions have z of 12 or more, and the largest z.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 4000000
s = [mix(i // 3) % 4 for i in range(n)]
z = [0] * n
l = r = 0
for i in range(1, n):
    k = min(r - i, z[i - l]) if i < r else 0
    while i + k < n and s[k] == s[i + k]:
        k += 1
    z[i] = k
    if i + k > r:
        l, r = i, i + k
print(sum(z), sum(1 for x in z if x >= 12), max(z))
