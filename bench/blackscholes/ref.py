# 20,000,000 Black-Scholes options from splitmix64 inputs (numpy, in
# chunks): the total of every call and put, each rounded to cents.
import numpy as np

def mix(v):
    x = (v + np.uint64(1)) * np.uint64(0x9E3779B97F4A7C15)
    x = (x ^ (x >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    x = (x ^ (x >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    return (x ^ (x >> np.uint64(31))) >> np.uint64(1)

def unit(v):
    return (mix(v) >> np.uint64(10)).astype(np.float64) / 9007199254740992.0

def cnd(x):
    k = 1.0 / (1.0 + 0.2316419 * np.abs(x))
    poly = k * (0.319381530 + k * (-0.356563782 + k * (1.781477937 + k * (-1.821255978 + k * 1.330274429))))
    w = 1.0 - 0.3989422804014327 * np.exp(-0.5 * x * x) * poly
    return np.where(x < 0.0, 1.0 - w, w)

def cents(x):  # half away from zero, like C's round
    return (np.sign(x) * np.floor(np.abs(x) * 100.0 + 0.5)).astype(np.int64)

n, total, step = 20_000_000, 0, 2_000_000
with np.errstate(over="ignore"):
    for lo in range(0, n, step):
        i = np.arange(lo, lo + step, dtype=np.uint64)
        f = np.uint64(5)
        s, k = 50.0 + 100.0 * unit(f * i), 50.0 + 100.0 * unit(f * i + np.uint64(1))
        t, r, v = 0.1 + 2.0 * unit(f * i + np.uint64(2)), 0.01 + 0.05 * unit(f * i + np.uint64(3)), 0.1 + 0.5 * unit(f * i + np.uint64(4))
        root = v * np.sqrt(t)
        d1 = (np.log(s / k) + (r + 0.5 * v * v) * t) / root
        d2 = d1 - root
        far = k * np.exp(-r * t)
        total += int(cents(s * cnd(d1) - far * cnd(d2)).sum() + cents(far * cnd(-d2) - s * cnd(-d1)).sum())
print(n, total)
