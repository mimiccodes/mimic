# The reference for intervals (pure Python): 2,000,000 intervals [s, s + l)
# with s = mix(2k) mod 10^9 and l = 1 + mix(2k + 1) mod 1,000 (splitmix64),
# sorted by start and merged where they overlap or touch. Prints the number
# of merged intervals, the total length they cover, and the longest.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


iv = sorted((mix(2 * k) % 10**9, 1 + mix(2 * k + 1) % 1000) for k in range(2000000))
merged = []
for s, l in iv:
    if merged and s <= merged[-1][1]:
        merged[-1][1] = max(merged[-1][1], s + l)
    else:
        merged.append([s, s + l])
lens = [e - s for s, e in merged]
print(len(merged), sum(lens), max(lens))
