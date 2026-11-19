# The reference for fenwick (Python): 1,048,576 counters; 1,000,000 updates
# (add 1 + mix(2k+1) % 1000 at position 1 + mix(2k) % 1,048,576, from
# splitmix64), then the prefix sums up to 1 + mix(3,000,000 + q) % 1,048,576
# for q below 2,000,000 (from running sums of the counters). Prints the sum
# of the prefix sums and the total.
from itertools import accumulate

M = (1 << 64) - 1
SIZE = 1048576


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


c = [0] * (SIZE + 1)
for k in range(1000000):
    c[1 + mix(2 * k) % SIZE] += 1 + mix(2 * k + 1) % 1000
run = list(accumulate(c))
total = sum(run[1 + mix(3000000 + q) % SIZE] for q in range(2000000))
print(total, run[SIZE])
