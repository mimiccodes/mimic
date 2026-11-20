# The reference for bwt (Python): 256 KiB of generated text in 32 blocks of
# 8 KiB; each block's Burrows-Wheeler transform (its rotations sorted, the
# last column and where the original sits), then move-to-front, then the
# block back from its transform. The checksum of each block's last column
# is added up, so blocks can be done in any order. Rotations are sorted directly here (the
# benchmark sorts them by prefix doubling).
B, blocks = 8192, 32
n = B * blocks
words = ["the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector",
         "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"]
text = bytearray()
i = 0
while len(text) < n:
    w = words[(i * 2654435761 >> 13) % 20 if i % 7 else (i * 40503 >> 7) % 20]
    text += w.encode() + (b". " if i % 11 == 10 else b" ")
    i += 1
d = bytes(text[:n])
primary = check = zeros = 0
same = 1
for k in range(blocks):
    blk = d[k * B:(k + 1) * B]
    dd = blk + blk
    sa = sorted(range(B), key=lambda i: dd[i:i + B])
    last = bytes(blk[(i - 1) % B] for i in sa)
    p = sa.index(0)
    primary += p
    c = 0
    for j, b in enumerate(last):
        c = (c * 31 + b + j) % 1000000007
    check = (check + c) % 1000000007
    # move-to-front
    table = list(range(256))
    for b in last:
        r = table.index(b)
        zeros += r == 0
        table.pop(r)
        table.insert(0, b)
    # back: the stable order of the last column maps each row to the next
    order = sorted(range(B), key=lambda j: (last[j], j))
    out = bytearray()
    j = order[p]
    for _ in range(B):
        out.append(last[j])
        j = order[j]
    same &= bytes(out) == blk
print(primary, check, zeros, same)
