# The reference for lis (pure Python with bisect): the longest strictly
# increasing subsequence of 4,000,000 values mix(k) mod 10^9 (splitmix64),
# by patience sorting: tails[l] is the smallest last value of an increasing
# subsequence of length l + 1; each value replaces the first tail not below
# it (a lower bound) or extends the list. Prints the length, and the sum of
# the tails.
from bisect import bisect_left

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


tails = []
for k in range(4000000):
    v = mix(k) % 10**9
    p = bisect_left(tails, v)
    if p == len(tails):
        tails.append(v)
    else:
        tails[p] = v
print(len(tails), sum(tails))
