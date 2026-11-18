# The reference for wordfreq (Python's re and Counter, not hashing): about
# 1,000,000 generated words (syllables chosen from a skewed draw of 6,000
# word numbers, from splitmix64) with spaces, commas, full stops and line
# breaks. Prints the words, the distinct words, and how often the most
# common one occurs.
import re
from collections import Counter

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def spelled(v):
    s = ""
    for k in range(1 + v % 4):
        s += "bcdfghklmnprstvz"[(v >> (3 * k + 2)) % 16] + "aeiou"[(v >> (3 * k + 7)) % 5]
    return s


text = []
for i in range(1000000):
    v = mix(2 * i) % (1 + mix(2 * i + 1) % 6000)
    sep = "\n" if i % 15 == 14 else ". " if i % 13 == 12 else ", " if i % 13 == 5 else " "
    text.append(spelled(v) + sep)
c = Counter(re.findall("[a-z]+", "".join(text)))
print(sum(c.values()), len(c), max(c.values()))
