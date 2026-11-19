// sudoku: 200 puzzles (full grids relabelled and emptied in part, from
// splitmix64) solved by backtracking: the first empty cell in reading
// order gets the digits 1 … 9 that fit, in order. Prints the sum over the
// puzzles of the top row of the solution read as a 9-digit number, and the
// number of placements tried in all.
#include <stdint.h>
#include <stdio.h>

#ifndef EMPTY
#define EMPTY 62
#endif

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int64_t tried = 0;

static int fits(const int *b, int i, int v) {
    int r = i / 9, c = i % 9;
    for (int k = 0; k < 9; k++)
        if (b[r * 9 + k] == v || b[k * 9 + c] == v) return 0;
    int br = r - r % 3, bc = c - c % 3;
    for (int dr = 0; dr < 3; dr++)
        for (int dc = 0; dc < 3; dc++)
            if (b[(br + dr) * 9 + bc + dc] == v) return 0;
    return 1;
}

static int solve(int *b) {
    int i = 0;
    while (i < 81 && b[i]) i++;
    if (i == 81) return 1;
    for (int v = 1; v <= 9; v++) {
        tried++;
        if (fits(b, i, v)) {
            b[i] = v;
            if (solve(b)) return 1;
            b[i] = 0;
        }
    }
    return 0;
}

int main(void) {
    int64_t total = 0;
    for (int64_t p = 0; p < 200; p++) {
        int perm[9], b[81];
        for (int i = 0; i < 9; i++) perm[i] = i + 1;
        for (int i = 8; i > 0; i--) {
            int j = (int)(mix(1000 * p + i) % (i + 1)), t = perm[i];
            perm[i] = perm[j];
            perm[j] = t;
        }
        for (int i = 0; i < 81; i++) {
            int r = i / 9, c = i % 9;
            b[i] = mix(81 * p + i) % 100 < EMPTY ? 0 : perm[(3 * (r % 3) + r / 3 + c) % 9];
        }
        solve(b);
        int64_t row = 0;
        for (int c = 0; c < 9; c++) row = row * 10 + b[c];
        total += row;
    }
    printf("%lld %lld\n", (long long)total, (long long)tried);
    return 0;
}
