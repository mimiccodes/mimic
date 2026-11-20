# The reference for totient (Python with numpy): Euler's phi for every
# number up to 10,000,000 by a sieve (phi starts as n; for each prime p,
# every multiple m loses phi[m] / p). Prints the sum of phi over 1 … n, how
# many n have phi(n) = n - 1 (the primes), and phi(10,000,000).
import numpy as np

n = 10000000
phi = np.arange(n + 1, dtype=np.int64)
for p in range(2, n + 1):
    if phi[p] == p:
        phi[p::p] -= phi[p::p] // p
print(int(phi[1:].sum()), int((phi[2:] == np.arange(1, n)).sum()), int(phi[n]))
