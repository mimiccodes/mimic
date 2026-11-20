// nqueens: the number of ways to put 15 queens on a 15x15 board so that
// none attacks another, by backtracking over rows with bit masks of the
// columns and both diagonals taken. Prints the count.
#include <stdint.h>
#include <stdio.h>

static int64_t solve(int n, int64_t cols, int64_t left, int64_t right) {
    int64_t all = ((int64_t)1 << n) - 1;
    if (cols == all) return 1;
    int64_t count = 0, free = all & ~(cols | left | right);
    while (free) {
        int64_t bit = free & -free;
        free ^= bit;
        count += solve(n, cols | bit, (left | bit) << 1, (right | bit) >> 1);
    }
    return count;
}

int main(void) {
    printf("%lld\n", (long long)solve(15, 0, 0, 0));
    return 0;
}
