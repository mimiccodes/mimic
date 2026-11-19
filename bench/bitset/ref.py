# The reference for bitset (Python with numpy): 100 sets over the numbers
# below 2^20, each as 16,384 words of 64 bits (word w of set s:
# mix(3(16,384 s + w)) & mix(3(…) + 1) & mix(3(…) + 2), so about one bit in
# eight is set; splitmix64, whose results are 63-bit). For every pair of
# sets, the sizes of their intersection and union. Prints the sum of the
# intersection sizes, the sum of the union sizes, and the pair with the
# largest intersection (first such in order) as i * 100 + j.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


S, W = 100, 16384
k = np.arange(S * W, dtype=np.uint64)
with np.errstate(over="ignore"):
    words = (mix(3 * k) & mix(3 * k + np.uint64(1)) & mix(3 * k + np.uint64(2))).reshape(S, W)


def popcount(a):
    return np.unpackbits(a.view(np.uint8)).sum()


inter = uni = 0
best, best_pair = -1, 0
for i in range(S):
    for j in range(i + 1, S):
        a = popcount(words[i] & words[j])
        inter += int(a)
        uni += int(popcount(words[i] | words[j]))
        if a > best:
            best, best_pair = a, i * 100 + j
print(inter, uni, best_pair)
