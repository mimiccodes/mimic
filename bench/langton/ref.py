# The reference for langton (pure Python): Langton's ant on a 1,024 × 1,024
# torus, all cells white at first, for 20,000,000 steps from the centre
# facing up: on white it turns right, on black left, flips the cell and
# moves one cell on. Prints the number of black cells at the end, and the
# ant's column, row and direction (0 up, 1 right, 2 down, 3 left).
L = 1024
grid = bytearray(L * L)
x = y = L // 2
d = 0
DX, DY = (0, 1, 0, -1), (-1, 0, 1, 0)
for _ in range(20000000):
    i = y * L + x
    d = (d + 1) % 4 if grid[i] == 0 else (d + 3) % 4
    grid[i] ^= 1
    x, y = (x + DX[d]) % L, (y + DY[d]) % L
print(sum(grid), x, y, d)
