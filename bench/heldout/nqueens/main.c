// Counts the ways to place N queens on an N x N board so that none attack
// each other, by backtracking row by row.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 12

static int64_t placed[N];

static int safe(int row, int64_t col) {
    for (int r = 0; r < row; r++) {
        if (placed[r] == col || llabs(placed[r] - col) == row - r) return 0;
    }
    return 1;
}

static int64_t solve(int row) {
    if (row == N) return 1;
    int64_t count = 0;
    for (int64_t c = 0; c < N; c++) {
        if (safe(row, c)) {
            placed[row] = c;
            count += solve(row + 1);
        }
    }
    return count;
}

int main(void) {
    printf("%lld\n", (long long)solve(0));
    return 0;
}
