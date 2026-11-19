// langton: Langton's ant on a 1,024 × 1,024 torus for 20,000,000 steps
// from the centre facing up. Prints the number of black cells at the end,
// and the ant's column, row and direction.
#include <stdint.h>
#include <stdio.h>

#define L 1024

int main(void) {
    static uint8_t grid[L * L];
    static const int DX[4] = {0, 1, 0, -1}, DY[4] = {-1, 0, 1, 0};
    int64_t x = L / 2, y = L / 2, d = 0;
    for (int64_t s = 0; s < 20000000; s++) {
        int64_t i = y * L + x;
        d = grid[i] ? (d + 3) % 4 : (d + 1) % 4;
        grid[i] ^= 1;
        x = (x + DX[d] + L) % L;
        y = (y + DY[d] + L) % L;
    }
    int64_t black = 0;
    for (int64_t i = 0; i < L * L; i++) black += grid[i];
    printf("%lld %lld %lld %lld\n", (long long)black, (long long)x, (long long)y, (long long)d);
    return 0;
}
