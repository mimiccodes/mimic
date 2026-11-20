# The reference for wordwrap (Python's textwrap): 2,000,000 words of 1 to
# 12 letters (word k: 1 + mix(k) mod 12 copies of 'a' + mix(k) mod 26;
# splitmix64) joined by spaces, wrapped greedily at 72 columns. Prints the
# number of lines, the sum over the lines of the square of the room left
# (72 - length), and the length of the longest line.
import textwrap

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


text = " ".join(chr(97 + mix(k) % 26) * (1 + mix(k) % 12) for k in range(2000000))
lines = textwrap.wrap(text, 72, break_long_words=False)
print(len(lines), sum((72 - len(l)) ** 2 for l in lines), max(len(l) for l in lines))
