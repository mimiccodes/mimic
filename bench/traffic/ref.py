# The reference for traffic (Python with numpy): the Nagel-Schreckenberg
# traffic model on a ring road of 1,000,000 cells with 200,000 cars (car c
# starts at cell 5c, speed 0) and top speed 5, for 500 steps. In each step
# every car at once speeds up by 1 (at most 5), slows to the gap to the car
# ahead, then with probability 1/4 slows by 1 (when mix(200,000 t + c) mod 4
# is 0, splitmix64), and moves on. Prints the total distance driven, the
# sum of the speeds at the end, and how many cars end standing.
import numpy as np

L, C, T = 1000000, 200000, 500


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


pos = np.arange(C, dtype=np.int64) * 5
v = np.zeros(C, dtype=np.int64)
c = np.arange(C, dtype=np.uint64)
driven = 0
for t in range(T):
    gap = (np.roll(pos, -1) - pos - 1) % L
    v = np.minimum(np.minimum(v + 1, 5), gap)
    with np.errstate(over="ignore"):
        slow = mix(np.uint64(C * t) + c) % np.uint64(4) == 0
    v = np.where(slow, np.maximum(v - 1, 0), v)
    pos = (pos + v) % L
    driven += int(v.sum())
print(driven, int(v.sum()), int((v == 0).sum()))
