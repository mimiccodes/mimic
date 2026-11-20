# The reference for bitpack (Python with numpy): 8,000,000 values of 13
# bits (mix(k) mod 8192, splitmix64) packed into 64-bit words, low bits
# first: value k takes bits 13k … 13k + 12 of the stream, bit j of the
# stream being bit j mod 64 of word j div 64. Then unpacked again. Prints
# the number of words, the sum of every word modulo 1,000,003 (as an
# unsigned number), and 1 when unpacking gives the values back.
import numpy as np


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


n, b = 8000000, 13
with np.errstate(over="ignore"):
    vals = mix(np.arange(n, dtype=np.uint64)) % np.uint64(1 << b)
bits = ((vals[:, None] >> np.arange(b, dtype=np.uint64)) & np.uint64(1)).astype(np.uint8).reshape(-1)
nw = (n * b + 63) // 64
bits = np.concatenate([bits, np.zeros(nw * 64 - len(bits), dtype=np.uint8)])
words = np.packbits(bits.reshape(nw, 64)[:, ::-1], axis=1).view(">u8").reshape(-1)
check = int((words % np.uint64(1000003)).sum())
back = (np.unpackbits(words.astype(">u8").view(np.uint8).reshape(nw, 8), axis=1)[:, ::-1].reshape(-1)[: n * b]).reshape(n, b)
same = np.array_equal((back.astype(np.uint64) << np.arange(b, dtype=np.uint64)).sum(axis=1), vals)
print(nw, check, 1 if same else 0)
