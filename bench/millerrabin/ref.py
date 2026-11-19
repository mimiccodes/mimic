# The reference for millerrabin (pure Python, sympy-free): how many of the
# 3,000,000 odd numbers just below 2^31 (2^31 - 1, 2^31 - 3, …) are prime,
# by the deterministic Miller-Rabin test with bases 2, 3, 5 and 7 (exact
# for every number below 3,215,031,751), and the sum of those primes.
# Python's pow does the modular powers.
def prime(n):
    d, s = n - 1, 0
    while d % 2 == 0:
        d //= 2
        s += 1
    for a in (2, 3, 5, 7):
        if a % n == 0:
            continue
        x = pow(a, d, n)
        if x in (1, n - 1):
            continue
        for _ in range(s - 1):
            x = x * x % n
            if x == n - 1:
                break
        else:
            return False
    return True


ps = [n for n in range((1 << 31) - 1, (1 << 31) - 6000000, -2) if prime(n)]
print(len(ps), sum(ps))
