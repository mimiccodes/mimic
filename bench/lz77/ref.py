# The reference for lz77 (Python): the same greedy matcher, independently
# written, to produce expected.txt.
n = 1 << 19
words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "array", "parallel",
         "mimic", "kernel", "fusion", "vector", "memory", "cache"]
text = []
i = 0
while len(text) < n:
    w = words[(i * 2654435761 >> 13) % 16]
    text.extend(w.encode() + b" ")
    i += 1
d = bytes(text[:n])
H = 1 << 16
table = [0] * H
p = lits = matches = mlen = 0
toks = []
while p < n:
    if p + 3 <= n:
        h = ((d[p] << 16 | d[p + 1] << 8 | d[p + 2]) * 2654435761 >> 15) & (H - 1)
        cand = table[h] - 1
        table[h] = p + 1
        if cand >= 0 and p - cand <= 65535:
            ln = 0
            while ln < 255 and p + ln < n and d[cand + ln] == d[p + ln]:
                ln += 1
            if ln >= 4:
                toks.append((ln, p - cand)); matches += 1; mlen += ln; p += ln
                continue
    toks.append((0, d[p])); lits += 1; p += 1
out = bytearray()
for a, b in toks:
    if a == 0:
        out.append(b)
    else:
        for _ in range(a):
            out.append(out[-b])
print(lits, matches, mlen, int(bytes(out) == d))
