# The reference for knapsack (Python): the best value from 2,000 items
# (weights 1 to 1,000, values 1 to 10,000, from splitmix64) in a knapsack of
# capacity 100,000, each item at most once. Prints the best value and the
# best at half the capacity.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, cap = 2000, 100000
best = [0] * (cap + 1)
for i in range(n):
    w, v = 1 + mix(2 * i) % 1000, 1 + mix(2 * i + 1) % 10000
    for c in range(cap, w - 1, -1):
        if best[c - w] + v > best[c]:
            best[c] = best[c - w] + v
print(best[cap], best[cap // 2])
