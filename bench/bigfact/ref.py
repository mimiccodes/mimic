# The reference for bigfact (Python's big integers): 20,000! (the benchmark
# multiplies limbs of 9 decimal digits, 2 … 20,000 one after another).
# Prints the number of decimal digits, their sum, and the number of
# trailing zeros.
import math
import sys

sys.set_int_max_str_digits(0)
s = str(math.factorial(20000))
print(len(s), sum(map(int, s)), len(s) - len(s.rstrip("0")))
