# The reference for kmp (Python's own str.find, not KMP): every
# occurrence, overlapping ones too, of 8 patterns in 4 MiB of generated
# text (words from a vocabulary). Prints each pattern's count.
n = 1 << 22
words = ["the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector",
         "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"]
text = bytearray()
i = 0
while len(text) < n:
    w = words[(i * 2654435761 >> 13) % 20 if i % 7 else (i * 40503 >> 7) % 20]
    text += w.encode() + (b". " if i % 11 == 10 else b" ")
    i += 1
t = bytes(text[:n]).decode()
patterns = ["the", "parallel array", "core loop", "a", "zebra quartz", "e the", "mimic", "of the of"]
counts = []
for p in patterns:
    c, at = 0, t.find(p)
    while at >= 0:
        c += 1
        at = t.find(p, at + 1)
    counts.append(c)
print(" ".join(map(str, counts)))
