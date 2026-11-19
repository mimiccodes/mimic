# The reference for anagram (pure Python): 500,000 words of 3 to 8 letters
# from the letters a-f (word k: 3 + mix(k) mod 6 letters, letter j being
# 'a' + mix(16k + j) mod 6; splitmix64), written as one text and split
# again, then grouped into anagram classes (words with the same letters).
# Prints the number of classes, the size of the largest, and the sum of the
# squares of the sizes.
from collections import Counter

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def word(k):
    return "".join(chr(97 + mix(16 * k + j) % 6) for j in range(3 + mix(k) % 6))


text = " ".join(word(k) for k in range(500000))
sizes = Counter("".join(sorted(w)) for w in text.split()).values()
print(len(sizes), max(sizes), sum(s * s for s in sizes))
