# The reference for regex (Python's own `re`, a different engine): how many
# of each pattern's 50,000 generated strings it matches in full.
import re

patterns = [("(a|b)*abb", "ab"), ("(0|1(01*0)*1)*", "01"), ("[a-c]*b[a-c]?c+", "abc"), ("(x|y|z)*(xy|yz)+z?", "xyz")]
mix = lambda v, m: (v * 2654435761 >> 11) % m
counts = []
for p, (pat, alpha) in enumerate(patterns):
    rx = re.compile(pat)
    c = 0
    for j in range(50000):
        n = 8 + mix(p * 7919 + j, 33)
        s = "".join(alpha[mix(p * 1000003 + j * 131 + k, len(alpha))] for k in range(n))
        c += rx.fullmatch(s) is not None
    counts.append(c)
print(" ".join(map(str, counts)))
