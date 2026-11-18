# The reference for aho (Python's own str.find, not Aho-Corasick): the
# same 4 MiB of generated text as kmp, and 45 patterns: the 20 words, 20
# two-word phrases and 5 short pieces that overlap the others. Prints the
# matches of all patterns (overlapping ones too) and the positions where
# at least one match ends.
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
patterns = words + [words[i] + " " + words[(i * 7 + 3) % 20] for i in range(20)] + ["he", "ar", "or", "e t", "ll"]
total, ends = 0, set()
for p in patterns:
    at = t.find(p)
    while at >= 0:
        total += 1
        ends.add(at + len(p))
        at = t.find(p, at + 1)
print(total, len(ends))
