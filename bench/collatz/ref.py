# The reference for collatz (Python): for every number below 3,000,000,
# the steps of its Collatz sequence until it reaches 1. Prints the number
# with the longest sequence (the smallest, on a tie), its steps, and the
# sum of all steps. (Each number's steps from those already known.)
n = 3000000
steps = [0] * (n + 1)
for i in range(2, n + 1):
    x, s = i, 0
    while x >= i:
        x = x // 2 if x % 2 == 0 else 3 * x + 1
        s += 1
    steps[i] = s + steps[x]
best = max(range(1, n + 1), key=lambda i: (steps[i], -i))
print(best, steps[best], sum(steps[1:]))
