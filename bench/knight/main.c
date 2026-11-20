// knight: the number of knight's tours (open or closed) on a 5 × 5 board
// from each square, by backtracking over the eight moves in a fixed order
// with the visited squares as a bitmask. Prints the total number of tours
// (1,728), the number from a corner (304), and the number of moves tried.
#include <stdint.h>
#include <stdio.h>

#define N 5

static const int DX[8] = {1, 2, 2, 1, -1, -2, -2, -1}, DY[8] = {2, 1, -1, -2, -2, -1, 1, 2};
static int64_t tried = 0;

static int64_t tours(int x, int y, int64_t mask, int depth) {
    if (depth == N * N) return 1;
    int64_t t = 0;
    for (int k = 0; k < 8; k++) {
        int nx = x + DX[k], ny = y + DY[k];
        tried++;
        if (nx < 0 || ny < 0 || nx >= N || ny >= N) continue;
        int64_t bit = (int64_t)1 << (ny * N + nx);
        if (mask & bit) continue;
        t += tours(nx, ny, mask | bit, depth + 1);
    }
    return t;
}

int main(void) {
    int64_t total = 0, corner = 0;
    for (int s = 0; s < N * N; s++) {
        int64_t t = tours(s % N, s / N, (int64_t)1 << s, 1);
        total += t;
        if (s == 0) corner = t;
    }
    printf("%lld %lld %lld\n", (long long)total, (long long)corner, (long long)tried);
    return 0;
}
