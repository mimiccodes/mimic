# The reference for numparse (pure Python): 2,000,000 signed integers of
# 1 to 16 digits (value i: mix(i) mod 10^(1 + mix(i + 10^7) mod 16), negative
# when mix(i + 2*10^7) is odd; splitmix64), written one a line, then read
# back. Prints the sum, the number of negatives, the largest and the
# smallest.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def value(i):
    v = mix(i) % 10 ** (1 + mix(i + 10**7) % 16)
    return -v if mix(i + 2 * 10**7) % 2 else v


text = "\n".join(str(value(i)) for i in range(2000000))
xs = [int(s) for s in text.split("\n")]
print(sum(xs), sum(1 for x in xs if x < 0), max(xs), min(xs))
