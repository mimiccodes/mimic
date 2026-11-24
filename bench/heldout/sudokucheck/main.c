// Generates 9 x 9 grids from a valid sudoku solution by shifting every row
// the same number of places (some also get two cells swapped) and checks
// each one: every row, column and box must hold 1-9 once. Prints how many
// grids are valid.
#include <stdint.h>
#include <stdio.h>

#define GRIDS 400000

static int64_t seed = 42;

static int64_t next(void) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed;
}

static int64_t base(int r, int c) { return (r * 3 + r / 3 + c) % 9 + 1; }

// cells holds 9 values; are they exactly 1..9?
static int group_ok(const int64_t *cells) {
    int seen[10] = {0};
    for (int i = 0; i < 9; i++) {
        int64_t v = cells[i];
        if (v < 1 || v > 9 || seen[v]) return 0;
        seen[v] = 1;
    }
    return 1;
}

static int valid(int64_t g[9][9]) {
    int64_t cells[9];
    for (int r = 0; r < 9; r++) {
        for (int c = 0; c < 9; c++) cells[c] = g[r][c];
        if (!group_ok(cells)) return 0;
    }
    for (int c = 0; c < 9; c++) {
        for (int r = 0; r < 9; r++) cells[r] = g[r][c];
        if (!group_ok(cells)) return 0;
    }
    for (int b = 0; b < 9; b++) {
        for (int i = 0; i < 9; i++) cells[i] = g[(b / 3) * 3 + i / 3][(b % 3) * 3 + i % 3];
        if (!group_ok(cells)) return 0;
    }
    return 1;
}

int main(void) {
    int64_t g[9][9];
    int64_t count = 0;
    for (int n = 0; n < GRIDS; n++) {
        int64_t k = next() % 9;
        for (int r = 0; r < 9; r++)
            for (int c = 0; c < 9; c++) g[r][c] = base(r, (c + k) % 9);
        if (next() % 4 == 0) {
            int64_t a = next() % 81;
            int64_t b = next() % 81;
            int64_t t = g[a / 9][a % 9];
            g[a / 9][a % 9] = g[b / 9][b % 9];
            g[b / 9][b % 9] = t;
        }
        if (valid(g)) count++;
    }
    printf("%lld\n", (long long)count);
    return 0;
}
