# The reference for soundex (pure Python): 1,000,000 names of 4 to 10
# letters (name k: letter j is 'a' + mix(16k + j) mod 26, the first one
# upper case; 4 + mix(k) mod 7 letters; splitmix64), each coded by American
# Soundex: the first letter, then the digits of the following consonants
# (b f p v: 1; c g j k q s x z: 2; d t: 3; l: 4; m n: 5; r: 6), adjacent
# equal digits once (h and w do not separate them; vowels do), padded with
# 0 to four characters. Prints how many distinct codes there are, the
# most common code and how often it occurs (the first such in sorted
# order), and the sum of the digits of all the codes.
from collections import Counter

M = (1 << 64) - 1
DIGIT = {c: d for d, cs in enumerate(["aeiouyhw", "bfpv", "cgjkqsxz", "dt", "l", "mn", "r"]) for c in cs}


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def name(k):
    s = "".join(chr(97 + mix(16 * k + j) % 26) for j in range(4 + mix(k) % 7))
    return s[0].upper() + s[1:]


def soundex(s):
    s = s.lower()
    out, last = s[0].upper(), DIGIT[s[0]]
    for c in s[1:]:
        d = DIGIT[c]
        if c in "hw":
            continue
        if d and d != last:
            out += str(d)
        last = d
        if len(out) == 4:
            break
    return out.ljust(4, "0")


codes = [soundex(name(k)) for k in range(1000000)]
cnt = Counter(codes)
top = max(sorted(cnt), key=lambda c: cnt[c])
print(len(cnt), top, cnt[top], sum(int(ch) for c in codes for ch in c[1:]))
