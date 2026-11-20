# The reference for heap (Python): 1,000,000 numbers below 1,000,000,000
# (mix(i) % 1e9, from splitmix64) pushed onto a min-heap and popped,
# smallest first. Prints the first, middle and last popped, and the sum of
# each value times its position (as a signed 64-bit number, wrapping).
import heapq

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


m = 1000000
h = []
for i in range(m):
    heapq.heappush(h, mix(i) % 1000000000)
s = [heapq.heappop(h) for _ in range(m)]
check = sum(v * i for i, v in enumerate(s)) & M
if check >= 1 << 63:
    check -= 1 << 64
print(s[0], s[m // 2], s[m - 1], check)
