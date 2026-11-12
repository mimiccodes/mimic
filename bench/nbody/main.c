#include <math.h>
#include <stdio.h>

/* 5 bodies, 1M steps of 0.01; each body's acceleration is the sum over the
 * other bodies in index order (the same order in every language) */
#define N 5
static const double PI = 3.141592653589793, DAYS = 365.24;

int main(void) {
    const double SM = 4 * PI * PI;
    double x[N] = {0, 4.84143144246472090e+00, 8.34336671824457987e+00, 1.28943695621391310e+01, 1.53796971148509165e+01};
    double y[N] = {0, -1.16032004402742839e+00, 4.12479856412430479e+00, -1.51111514016986312e+01, -2.59193146099879641e+01};
    double z[N] = {0, -1.03622044471123109e-01, -4.03523417114321381e-01, -2.23307578892655734e-01, 1.79258772950371181e-01};
    double vx[N] = {0, 1.66007664274403694e-03 * DAYS, -2.76742510726862411e-03 * DAYS, 2.96460137564761618e-03 * DAYS, 2.68067772490389322e-03 * DAYS};
    double vy[N] = {0, 7.69901118419740425e-03 * DAYS, 4.99852801234917238e-03 * DAYS, 2.37847173959480950e-03 * DAYS, 1.62824170038242295e-03 * DAYS};
    double vz[N] = {0, -6.90460016972063023e-05 * DAYS, 2.30417297573763929e-05 * DAYS, -2.96589568540237556e-05 * DAYS, -9.51592254519715870e-05 * DAYS};
    double m[N] = {SM, 9.54791938424326609e-04 * SM, 2.85885980666130812e-04 * SM, 4.36624404335156298e-05 * SM, 5.15138902046611451e-05 * SM};
    double px = 0, py = 0, pz = 0;
    for (int i = 0; i < N; i++) { px += vx[i] * m[i]; py += vy[i] * m[i]; pz += vz[i] * m[i]; }
    vx[0] = -px / SM; vy[0] = -py / SM; vz[0] = -pz / SM;
    for (int pass = 0; pass < 2; pass++) {
        double e = 0;
        for (int i = 0; i < N; i++) {
            e += 0.5 * m[i] * (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]);
            for (int j = i + 1; j < N; j++) {
                double dx = x[i] - x[j], dy = y[i] - y[j], dz = z[i] - z[j];
                e -= m[i] * m[j] / sqrt(dx * dx + dy * dy + dz * dz);
            }
        }
        printf("%.9f\n", e);
        if (pass == 1) break;
        for (int s = 0; s < 1000000; s++) {
            double ax[N], ay[N], az[N];
            for (int i = 0; i < N; i++) {
                ax[i] = ay[i] = az[i] = 0;
                for (int j = 0; j < N; j++) {
                    if (j == i) continue;
                    double dx = x[j] - x[i], dy = y[j] - y[i], dz = z[j] - z[i];
                    double d2 = dx * dx + dy * dy + dz * dz, mag = m[j] / (d2 * sqrt(d2));
                    ax[i] += dx * mag; ay[i] += dy * mag; az[i] += dz * mag;
                }
            }
            for (int i = 0; i < N; i++) {
                vx[i] += 0.01 * ax[i]; vy[i] += 0.01 * ay[i]; vz[i] += 0.01 * az[i];
                x[i] += 0.01 * vx[i]; y[i] += 0.01 * vy[i]; z[i] += 0.01 * vz[i];
            }
        }
    }
    return 0;
}
