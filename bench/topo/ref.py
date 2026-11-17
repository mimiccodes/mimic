# The reference for topo (Python, Kahn's algorithm by levels): the same
# random graph with each edge pointing from the smaller node to the larger
# (a DAG; an edge to itself left out), every second edge only. Nodes go
# out level by level (all whose incoming edges are gone), ascending within
# a level. Prints the levels and a checksum of the order.
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 200000, 1000000
src = [mix(2 * e + 1) % n for e in range(m)]
dst = [mix(2 * e + 2) % n for e in range(m)]

indeg = [0] * n
out = [[] for _ in range(n)]
for e in range(0, m, 2):
    a, b = min(src[e], dst[e]), max(src[e], dst[e])
    if a != b:
        out[a].append(b)
        indeg[b] += 1
level = [v for v in range(n) if indeg[v] == 0]
levels = pos = check = 0
while level:
    levels += 1
    nxt = []
    for v in level:
        check = (check + pos * v) % 1000000007
        pos += 1
        for w in out[v]:
            indeg[w] -= 1
            if indeg[w] == 0:
                nxt.append(w)
    level = sorted(nxt)
print(levels, check)
