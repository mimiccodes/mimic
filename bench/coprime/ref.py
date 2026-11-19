# The reference for coprime (Python with numpy): over all pairs (i, j) with
# 1 <= i, j <= 4,000, how many have gcd 1, and the sum of all the gcds.
import numpy as np

n = 4000
i = np.arange(1, n + 1, dtype=np.int64)
g = np.gcd.outer(i, i)
print(int((g == 1).sum()), int(g.sum()))
