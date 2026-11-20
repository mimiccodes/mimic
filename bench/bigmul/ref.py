# The product of two 120,000-digit splitmix64 numbers (30,000 limbs of four
# digits), by Python's own big integers: its limbs, their sum, the top limb
# and the middle one.
import sys

sys.set_int_max_str_digits(0)
M = (1 << 64) - 1

def mix(v):
    x = ((v + 1) * 0x9E3779B97F4A7C15) & M
    x = ((x ^ x >> 30) * 0xBF58476D1CE4E5B9) & M
    x = ((x ^ x >> 27) * 0x94D049BB133111EB) & M
    return (x ^ x >> 31) >> 1

n = 30000
def number(seed):
    limbs = [1 + mix(seed + i) % 9999 if i == n - 1 else mix(seed + i) % 10000 for i in range(n)]
    return int("".join(f"{x:04d}" for x in reversed(limbs)))

s = str(number(0) * number(n))
s = s.zfill((len(s) + 3) // 4 * 4)
c = [int(s[len(s) - 4 * (k + 1) : len(s) - 4 * k]) for k in range(len(s) // 4)]
print(len(c), sum(c), c[-1], c[n])
