# The reference for lzw (pure Python): 4,000,000 bytes, byte i being
# 97 + mix(i div 4 + mix(i div 64) mod 3) mod 5 (splitmix64: letters a-e in
# runs of four, with repeats), compressed by LZW with 12-bit codes (codes
# 0-255 are the bytes, new strings get 256, 257, … up to 4095, then the
# dictionary stays as it is), and decompressed again. Prints the number of
# codes, a checksum that weighs every code by (i mod 251 + 1), and 1 when
# decompressing gives the bytes back.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n = 4000000
data = bytes(97 + mix(i // 4 + mix(i // 64) % 3) % 5 for i in range(n))
table, nxt, codes = {}, 256, []
w = data[0]
for ch in data[1:]:
    k = table.get((w, ch))
    if k is not None:
        w = k
    else:
        codes.append(w)
        if nxt < 4096:
            table[(w, ch)] = nxt
            nxt += 1
        w = ch
codes.append(w)
check = sum(c * (i % 251 + 1) for i, c in enumerate(codes))
strings = [bytes([b]) for b in range(256)]
out = bytearray(strings[codes[0]])
prev = codes[0]
for c in codes[1:]:
    s = strings[c] if c < len(strings) else strings[prev] + strings[prev][:1]
    if len(strings) < 4096:
        strings.append(strings[prev] + s[:1])
    out += s
    prev = c
print(len(codes), check, 1 if out == data else 0)
