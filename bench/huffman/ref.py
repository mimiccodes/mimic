# The reference for huffman (Python): byte counts, a Huffman tree merged in
# a fixed order (by weight, then by node number: leaves are 0-255 by byte,
# merged nodes are numbered from 256 as they are made), code lengths, the
# canonical codes, the encoded size in bits, a checksum of the packed
# 64-bit words, and the decoded bytes compared with the original.
import heapq

n = 1 << 18
text = bytearray()
i = 0
words = ["the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector",
         "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"]
while len(text) < n:
    w = words[(i * 2654435761 >> 13) % 20 if i % 7 else (i * 40503 >> 7) % 20]
    text += w.encode() + (b". " if i % 11 == 10 else b" ")
    i += 1
d = bytes(text[:n])

count = [0] * 256
for b in d:
    count[b] += 1
heap = [(count[b], b) for b in range(256) if count[b] > 0]
heapq.heapify(heap)
parent = {}
nxt = 256
while len(heap) > 1:
    w1, a = heapq.heappop(heap)
    w2, b = heapq.heappop(heap)
    parent[a] = nxt
    parent[b] = nxt
    heapq.heappush(heap, (w1 + w2, nxt))
    nxt += 1
length = [0] * 256
for b in range(256):
    if count[b]:
        k, l = b, 0
        while k in parent:
            k = parent[k]
            l += 1
        length[b] = l
# canonical codes: by (length, byte)
code = [0] * 256
c = 0
prev = 0
for l, b in sorted((length[b], b) for b in range(256) if count[b]):
    c <<= l - prev
    code[b] = c
    c += 1
    prev = l
bits = sum(count[b] * length[b] for b in range(256))
# pack, most significant bit first, into 64-bit words
words64 = [0] * ((bits + 63) // 64)
pos = 0
for b in d:
    for k in range(length[b] - 1, -1, -1):
        if code[b] >> k & 1:
            words64[pos // 64] |= 1 << (63 - pos % 64)
        pos += 1
check = 0
for w in words64:
    check = (check * 31 + w) % 1000000007
# decode bit by bit with the canonical table
table = {(length[b], code[b]): b for b in range(256) if count[b]}
out = bytearray()
cur = l = 0
for p in range(bits):
    cur = cur << 1 | (words64[p // 64] >> (63 - p % 64) & 1)
    l += 1
    if (l, cur) in table:
        out.append(table[(l, cur)])
        cur = l = 0
print(bits, max(length), check, int(bytes(out) == d))
