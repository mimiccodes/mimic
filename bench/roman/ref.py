# The reference for roman (pure Python): 1,000,000 numbers 1 + mix(k) mod
# 3,999 (splitmix64) written as Roman numerals (greedily from M, CM, D, CD,
# C, XC, L, XL, X, IX, V, IV, I) and read back (a symbol smaller than the
# next one is subtracted). Prints the total length of the numerals, the sum
# of the values read back, and how many numerals contain "MM".
M = (1 << 64) - 1
SYMS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
VAL = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100, "D": 500, "M": 1000}


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def roman(n):
    out = []
    for v, s in SYMS:
        while n >= v:
            out.append(s)
            n -= v
    return "".join(out)


def value(r):
    t = 0
    for i, c in enumerate(r):
        v = VAL[c]
        t += -v if i + 1 < len(r) and v < VAL[r[i + 1]] else v
    return t


rs = [roman(1 + mix(k) % 3999) for k in range(1000000)]
print(sum(len(r) for r in rs), sum(value(r) for r in rs), sum(1 for r in rs if "MM" in r))
