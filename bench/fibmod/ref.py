# The reference for fibmod (pure Python): F(n) modulo 1,000,000,007 for
# 500,000 values n = mix(k) mod 10^18 (splitmix64), by fast doubling over
# the bits of n from the top: with (a, b) = (F(m), F(m + 1)),
# F(2m) = a (2b - a) and F(2m + 1) = a^2 + b^2. Prints the sum of the
# results modulo the prime, and their exclusive or.
M = (1 << 64) - 1
P = 1000000007


def mix(v):
    x = (v + 1) * 0x9E3779B97F4A7C15 & M
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9 & M
    x = (x ^ x >> 27) * 0x94D049BB133111EB & M
    return (x ^ x >> 31) >> 1


def fib(n):
    a, b = 0, 1
    for bit in range(n.bit_length() - 1, -1, -1):
        c = a * (2 * b - a) % P
        d = (a * a + b * b) % P
        a, b = (d, (c + d) % P) if n >> bit & 1 else (c, d)
    return a


s = x = 0
for k in range(500000):
    f = fib(mix(k) % 10**18)
    s = (s + f) % P
    x ^= f
print(s, x)
