# The reference for components (Python, union-find): the bfs random graph
# at half the size, taken as undirected, with every third edge left out so
# it falls into many pieces. Prints the components and the size of the
# largest.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 100000, 500000
src = [mix(2 * e + 1) % n for e in range(m)]
dst = [mix(2 * e + 2) % n for e in range(m)]

parent = list(range(n))


def find(x):
    while parent[x] != x:
        x = parent[x]
    return x


for e in range(m):
    if e % 3 != 0:
        a, b = find(src[e]), find(dst[e])
        if a != b:
            parent[max(a, b)] = min(a, b)
sizes = {}
for v in range(n):
    r = find(v)
    sizes[r] = sizes.get(r, 0) + 1
print(len(sizes), max(sizes.values()))
