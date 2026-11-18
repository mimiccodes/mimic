# The reference for trie (Python's own set): 200,000 generated words (4
# to 14 letters of a 4-letter alphabet, from splitmix64) put in a trie,
# then 200,000 other words looked up, half of them from the same range.
# Prints the trie's nodes (every distinct prefix, the empty one too), the
# lookups found as words, and those found as prefixes.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def word(k):
    return tuple(mix(k * 1000 + i) % 4 for i in range(4 + mix(k * 31) % 11))


words = {word(k) for k in range(200000)}
prefixes = {w[:i] for w in words for i in range(len(w) + 1)}
queries = [word(k) for k in range(100000, 300000)]
print(len(prefixes), sum(q in words for q in queries), sum(q in prefixes for q in queries))
