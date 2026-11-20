# The reference for ising (Python with numpy): the 2D Ising model on a
# 512 × 512 torus at β = 0.44, 100 sweeps of Metropolis updates in
# checkerboard order (the sites with (x + y) even, then odd). Spins start
# at +1 where mix(i) is odd (splitmix64), else -1. A site's flip costs
# dE = 2 s (sum of its four neighbours); it flips when dE <= 0, or when
# r < threshold[dE], r = mix(2^30 + (2 * sweep + colour) * 512^2 + i) mod 2^32,
# with the thresholds floor(exp(-β dE) 2^32) fixed: 738,927,063 for dE = 4
# and 127,128,606 for dE = 8. Prints the magnetisation (sum of spins) and
# the energy (minus the sum over bonds of s s') at the end.
import numpy as np

L = 512
N = L * L


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


i = np.arange(N, dtype=np.uint64)
with np.errstate(over="ignore"):
    s = np.where(mix(i) % np.uint64(2) == 1, 1, -1).astype(np.int64).reshape(L, L)
yy, xx = np.indices((L, L))
colour_of = (xx + yy) % 2
thr = {4: 738927063, 8: 127128606}
for sweep in range(100):
    for colour in (0, 1):
        nb = np.roll(s, 1, 0) + np.roll(s, -1, 0) + np.roll(s, 1, 1) + np.roll(s, -1, 1)
        de = 2 * s * nb
        with np.errstate(over="ignore"):
            r = (mix(np.uint64(1 << 30) + np.uint64((2 * sweep + colour) * N) + i) % np.uint64(1 << 32)).astype(np.int64).reshape(L, L)
        t = np.where(de == 4, thr[4], np.where(de == 8, thr[8], 0))
        flip = (colour_of == colour) & ((de <= 0) | (r < t))
        s = np.where(flip, -s, s)
energy = -int((s * np.roll(s, 1, 0)).sum() + (s * np.roll(s, 1, 1)).sum())
print(int(s.sum()), energy)
