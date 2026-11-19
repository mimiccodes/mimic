# The reference for partitions (pure Python): the partition numbers p(n)
# modulo 1,000,000,007 for n up to 200,000, by Euler's pentagonal number
# recurrence: p(n) = sum over k >= 1 of (-1)^(k+1) (p(n - k(3k-1)/2) +
# p(n - k(3k+1)/2)), terms with a negative argument left out. Prints
# p(200,000) and the sum of all p(n) modulo the prime.
P = 1000000007
n = 200000
p = [1] + [0] * n
for m in range(1, n + 1):
    s, k = 0, 1
    while True:
        g1 = k * (3 * k - 1) // 2
        if g1 > m:
            break
        g2 = k * (3 * k + 1) // 2
        t = p[m - g1] + (p[m - g2] if g2 <= m else 0)
        s = s + t if k % 2 else s - t
        k += 1
    p[m] = s % P
print(p[n], sum(p) % P)
