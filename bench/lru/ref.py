# The reference for lru (Python's OrderedDict): an LRU cache of 10,000
# entries over 4,000,000 accesses to keys below 100,000; access t asks for
# key mix(t) mod 100,000 when mix(t + 10^7) mod 4 is 0, else for a "hot" key
# mix(t) mod 2,000 (splitmix64). A miss adds the key, dropping the least
# recently used one when full. Prints the hits, the misses, and the sum of
# the keys left in the cache.
from collections import OrderedDict

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


cache = OrderedDict()
hits = misses = 0
for t in range(4000000):
    k = mix(t) % 100000 if mix(t + 10**7) % 4 == 0 else mix(t) % 2000
    if k in cache:
        hits += 1
        cache.move_to_end(k)
    else:
        misses += 1
        cache[k] = True
        if len(cache) > 10000:
            cache.popitem(last=False)
print(hits, misses, sum(cache))
