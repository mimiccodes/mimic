# The reference for lisp (pure Python): 50,000 random s-expressions such as
# (+ (* 3 4) (max 2 (- 7 9))), generated from splitmix64 (node with seed s
# at depth d: a number mix(s + 1) mod 100 when d is 0 or mix(s) mod 3 is 0,
# else (op a b) with op one of + - * max min by mix(s + 2) mod 5 and
# children seeded mix(s + 3) and mix(s + 4); expression k has seed k and
# depth 7), written as text, tokenised and evaluated with all arithmetic
# modulo 1,000,000,007. Prints the number of tokens, the sum of the values
# modulo p, and the largest value.
import sys

sys.setrecursionlimit(10000)
M = (1 << 64) - 1
P = 1000000007
OPS = ["+", "-", "*", "max", "min"]


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def gen(s, d):
    if d == 0 or mix(s) % 3 == 0:
        return str(mix(s + 1) % 100)
    return f"({OPS[mix(s + 2) % 5]} {gen(mix(s + 3), d - 1)} {gen(mix(s + 4), d - 1)})"


text = "\n".join(gen(k, 7) for k in range(50000))
toks = text.replace("(", " ( ").replace(")", " ) ").split()


def ev(i):
    t = toks[i]
    if t != "(":
        return int(t), i + 1
    op = toks[i + 1]
    a, i = ev(i + 2)
    b, i = ev(i)
    v = {"+": (a + b) % P, "-": (a - b) % P, "*": a * b % P, "max": max(a, b), "min": min(a, b)}[op]
    return v, i + 1


vals, i = [], 0
while i < len(toks):
    v, i = ev(i)
    vals.append(v)
print(len(toks), sum(vals) % P, max(vals))
