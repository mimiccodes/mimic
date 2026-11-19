# The reference for polyeval (Python with numpy): 1,000,000 polynomials of
# degree 31 modulo 1,000,000,007 (coefficient j of polynomial k:
# mix(32k + j) mod p, highest first; splitmix64), each evaluated at
# x = mix(10^8 + k) mod p by Horner's rule. Prints the sum of the values
# modulo p and their exclusive or.
import numpy as np

P = 1000000007


def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ x >> np.uint64(30)) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ x >> np.uint64(27)) * np.uint64(0x94D049BB133111EB)
    return (x ^ x >> np.uint64(31)) >> np.uint64(1)


n = 1000000
k = np.arange(n, dtype=np.uint64)
with np.errstate(over="ignore"):
    x = (mix(np.uint64(10**8) + k) % np.uint64(P)).astype(np.int64)
    v = np.zeros(n, dtype=np.int64)
    for j in range(32):
        c = (mix(np.uint64(32) * k + np.uint64(j)) % np.uint64(P)).astype(np.int64)
        v = (v * x % P + c) % P
print(int(v.sum() % P), int(np.bitwise_xor.reduce(v)))
