// segments: 8,000 short segments on a 100,000 × 100,000 grid (from
// splitmix64), every pair tested for meeting by orientations (collinear
// touching counts). Prints the number of pairs that meet, and the sum of
// i + j over them.
#include <stdint.h>
#include <stdio.h>

#define N 8000

static int64_t mix(int64_t v) {
    uint64_t x = (uint64_t)(v + 1) * 0x9E3779B97F4A7C15ull;
    x = (x ^ x >> 30) * 0xBF58476D1CE4E5B9ull;
    x = (x ^ x >> 27) * 0x94D049BB133111EBull;
    return (int64_t)((x ^ x >> 31) >> 1);
}

static int orient(int64_t px, int64_t py, int64_t qx, int64_t qy, int64_t rx, int64_t ry) {
    int64_t c = (qx - px) * (ry - py) - (qy - py) * (rx - px);
    return (c > 0) - (c < 0);
}

#define MIN(a, b) ((a) < (b) ? (a) : (b))
#define MAX(a, b) ((a) > (b) ? (a) : (b))
static int on(int64_t px, int64_t py, int64_t qx, int64_t qy, int64_t rx, int64_t ry) {
    return MIN(px, qx) <= rx && rx <= MAX(px, qx) && MIN(py, qy) <= ry && ry <= MAX(py, qy);
}

static int64_t ax[N], ay[N], bx[N], by[N];

static int meet(int i, int j) {
    int o1 = orient(ax[i], ay[i], bx[i], by[i], ax[j], ay[j]);
    int o2 = orient(ax[i], ay[i], bx[i], by[i], bx[j], by[j]);
    int o3 = orient(ax[j], ay[j], bx[j], by[j], ax[i], ay[i]);
    int o4 = orient(ax[j], ay[j], bx[j], by[j], bx[i], by[i]);
    return (o1 != o2 && o3 != o4) || (o1 == 0 && on(ax[i], ay[i], bx[i], by[i], ax[j], ay[j])) ||
           (o2 == 0 && on(ax[i], ay[i], bx[i], by[i], bx[j], by[j])) || (o3 == 0 && on(ax[j], ay[j], bx[j], by[j], ax[i], ay[i])) ||
           (o4 == 0 && on(ax[j], ay[j], bx[j], by[j], bx[i], by[i]));
}

int main(void) {
    for (int64_t k = 0; k < N; k++) {
        ax[k] = mix(4 * k) % 100000;
        ay[k] = mix(4 * k + 1) % 100000;
        bx[k] = ax[k] + mix(4 * k + 2) % 4001 - 2000;
        by[k] = ay[k] + mix(4 * k + 3) % 4001 - 2000;
    }
    int64_t count = 0, total = 0;
    for (int i = 0; i < N; i++)
        for (int j = i + 1; j < N; j++)
            if (meet(i, j)) {
                count++;
                total += i + j;
            }
    printf("%lld %lld\n", (long long)count, (long long)total);
    return 0;
}
