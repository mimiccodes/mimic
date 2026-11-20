# The reference for coins (Python with numpy): the number of ways, modulo
# 1,000,000,007, to make every amount up to 3,000,000 from 30 kinds of
# coin, k*k + 1 for k below 30 (1, 2, 5, 10, …, 842), each kind used any
# number of times. Coin by coin, ways[x] += ways[x - c] for x from c up
# (done here per residue of x mod c, as a running sum). Prints the ways for
# 3,000,000 and the sum of all the ways (each modulo 1,000,000,007).
import numpy as np

A, P = 3000000, 1000000007
ways = np.zeros(A + 1, dtype=np.int64)
ways[0] = 1
for k in range(30):
    c = k * k + 1
    for r in range(c):
        s = ways[r::c]
        # running sum modulo P, without overflow: in blocks
        out = np.empty_like(s)
        carry = 0
        for lo in range(0, len(s), 4096):
            blk = np.cumsum(s[lo:lo + 4096]) + carry
            out[lo:lo + 4096] = blk % P
            carry = int(out[min(lo + 4095, len(s) - 1)])
        ways[r::c] = out
print(int(ways[A]), int(ways.sum() % P))
