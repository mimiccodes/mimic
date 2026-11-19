# The reference for mst (Python with scipy-free union-find): Kruskal's
# minimum spanning forest of a random graph of 200,000 nodes and 1,000,000
# edges (edge e joins mix(3e) mod n and mix(3e + 1) mod n with weight
# mix(3e + 2) mod 1,000,000; splitmix64). Edges are taken by weight, then
# by number; union-find by size. Prints the total weight of the forest, its
# number of edges, and the sum of the numbers of the edges taken.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 200000, 1000000
edges = sorted((mix(3 * e + 2) % 1000000, e) for e in range(m))
parent = list(range(n))
size = [1] * n


def find(x):
    while parent[x] != x:
        x = parent[x]
    return x


total = count = ids = 0
for w, e in edges:
    a, b = find(mix(3 * e) % n), find(mix(3 * e + 1) % n)
    if a == b:
        continue
    if size[a] < size[b]:
        a, b = b, a
    parent[b] = a
    size[a] += size[b]
    total += w
    count += 1
    ids += e
print(total, count, ids)
