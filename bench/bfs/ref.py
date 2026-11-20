# The reference for bfs (Python): a random graph of 200,000 nodes and
# 1,000,000 directed edges (u, v from splitmix64 of the edge number); breadth-
# first distances from node 0 with a queue. Prints the nodes reached, the
# sum of their distances and the largest.
from collections import deque

n, m = 200000, 1000000
M = (1 << 64) - 1


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


adj = [[] for _ in range(n)]
for e in range(m):
    adj[mix(2 * e + 1) % n].append(mix(2 * e + 2) % n)
dist = [-1] * n
dist[0] = 0
q = deque([0])
while q:
    u = q.popleft()
    for v in adj[u]:
        if dist[v] < 0:
            dist[v] = dist[u] + 1
            q.append(v)
reached = [d for d in dist if d >= 0]
print(len(reached), sum(reached), max(reached))
