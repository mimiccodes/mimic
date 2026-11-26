// Conway's Game of Life on a 300 x 300 grid (cells outside are dead) for
// 300 generations; prints the number of live cells.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define N 300
#define GENS 300

static int64_t grid[N][N], next_grid[N][N];

int main(void) {
    int64_t x = 42;
    for (int r = 0; r < N; r++)
        for (int c = 0; c < N; c++) {
            x = (x * 1103515245 + 12345) % 2147483648;
            grid[r][c] = (x % 3 == 0) ? 1 : 0;
        }
    for (int g = 0; g < GENS; g++) {
        for (int r = 0; r < N; r++)
            for (int c = 0; c < N; c++) {
                int64_t n = 0;
                for (int dr = -1; dr <= 1; dr++)
                    for (int dc = -1; dc <= 1; dc++) {
                        if (dr == 0 && dc == 0) continue;
                        int rr = r + dr, cc = c + dc;
                        if (rr >= 0 && rr < N && cc >= 0 && cc < N) n += grid[rr][cc];
                    }
                if (grid[r][c] == 1) next_grid[r][c] = (n == 2 || n == 3) ? 1 : 0;
                else next_grid[r][c] = (n == 3) ? 1 : 0;
            }
        for (int r = 0; r < N; r++)
            for (int c = 0; c < N; c++) grid[r][c] = next_grid[r][c];
    }
    int64_t live = 0;
    for (int r = 0; r < N; r++)
        for (int c = 0; c < N; c++) live += grid[r][c];
    printf("%lld\n", (long long)live);
    return 0;
}
