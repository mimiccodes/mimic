# The reference for bipartite (pure Python): a random undirected graph of
# 300,000 nodes and 200,000 edges (edge e joins mix(2e) and mix(2e + 1)
# mod n; splitmix64), each component two-coloured by breadth-first search
# from its smallest node (neighbours in the order of the edge list). Prints
# the number of components, how many of them are bipartite, and the size of
# the largest.
from collections import deque

M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


n, m = 300000, 200000
adj = [[] for _ in range(n)]
for e in range(m):
    a, b = mix(2 * e) % n, mix(2 * e + 1) % n
    adj[a].append(b)
    adj[b].append(a)
colour = [-1] * n
comps = bip = largest = 0
for s in range(n):
    if colour[s] >= 0:
        continue
    comps += 1
    colour[s] = 0
    q, ok, size = deque([s]), True, 0
    while q:
        u = q.popleft()
        size += 1
        for v in adj[u]:
            if colour[v] < 0:
                colour[v] = 1 - colour[u]
                q.append(v)
            elif colour[v] == colour[u]:
                ok = False
    bip += ok
    largest = max(largest, size)
print(comps, bip, largest)
