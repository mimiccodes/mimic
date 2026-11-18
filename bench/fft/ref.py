# The product of two polynomials of 524,288 splitmix64 coefficients (0 to
# 99), by numpy's FFT, rounded: the sum of its coefficients, a checksum and
# the middle one. The sum is also checked exactly: it is sum(a) × sum(b).
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

n = 1 << 20
half = n // 2
with np.errstate(over="ignore"):
    i = np.arange(half, dtype=np.uint64)
    a = (mix(i) % np.uint64(100)).astype(np.int64)
    b = (mix(np.uint64(n) + i) % np.uint64(100)).astype(np.int64)
c = np.rint(np.fft.irfft(np.fft.rfft(a, n) * np.fft.rfft(b, n), n)).astype(np.int64)
assert int(c.sum()) == int(a.sum()) * int(b.sum())
assert int(c[123]) == int(sum(int(a[k]) * int(b[123 - k]) for k in range(124)))
check = int((c % 1000 * (np.arange(n) % 1000)).sum())
print(int(c.sum()), check, int(c[half - 1]))
