# The primes below 100,000,000 (a plain sieve): their count and the largest.
n = 100_000_000
f = bytearray([1]) * n
f[0] = f[1] = 0
for p in range(2, int(n**0.5) + 1):
    if f[p]:
        f[p * p :: p] = bytes(len(range(p * p, n, p)))
print(f.count(1), f.rindex(1))
