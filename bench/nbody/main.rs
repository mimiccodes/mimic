// 5 bodies, 1M steps of 0.01; each body's acceleration is the sum over the
// other bodies in index order (the same order in every language)
const N: usize = 5;

fn energy(x: &[f64; N], y: &[f64; N], z: &[f64; N], vx: &[f64; N], vy: &[f64; N], vz: &[f64; N], m: &[f64; N]) -> f64 {
    let mut e = 0.0;
    for i in 0..N {
        e += 0.5 * m[i] * (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]);
        for j in i + 1..N {
            let (dx, dy, dz) = (x[i] - x[j], y[i] - y[j], z[i] - z[j]);
            e -= m[i] * m[j] / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}

fn main() {
    let pi = 3.141592653589793f64;
    let days = 365.24;
    let sm = 4.0 * pi * pi;
    let mut x = [0.0, 4.84143144246472090e+00, 8.34336671824457987e+00, 1.28943695621391310e+01, 1.53796971148509165e+01];
    let mut y = [0.0, -1.16032004402742839e+00, 4.12479856412430479e+00, -1.51111514016986312e+01, -2.59193146099879641e+01];
    let mut z = [0.0, -1.03622044471123109e-01, -4.03523417114321381e-01, -2.23307578892655734e-01, 1.79258772950371181e-01];
    let mut vx = [0.0, 1.66007664274403694e-03 * days, -2.76742510726862411e-03 * days, 2.96460137564761618e-03 * days, 2.68067772490389322e-03 * days];
    let mut vy = [0.0, 7.69901118419740425e-03 * days, 4.99852801234917238e-03 * days, 2.37847173959480950e-03 * days, 1.62824170038242295e-03 * days];
    let mut vz = [0.0, -6.90460016972063023e-05 * days, 2.30417297573763929e-05 * days, -2.96589568540237556e-05 * days, -9.51592254519715870e-05 * days];
    let m = [sm, 9.54791938424326609e-04 * sm, 2.85885980666130812e-04 * sm, 4.36624404335156298e-05 * sm, 5.15138902046611451e-05 * sm];
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for i in 0..N {
        px += vx[i] * m[i];
        py += vy[i] * m[i];
        pz += vz[i] * m[i];
    }
    vx[0] = -px / sm;
    vy[0] = -py / sm;
    vz[0] = -pz / sm;
    println!("{:.9}", energy(&x, &y, &z, &vx, &vy, &vz, &m));
    for _ in 0..1_000_000 {
        let (mut ax, mut ay, mut az) = ([0.0; N], [0.0; N], [0.0; N]);
        for i in 0..N {
            for j in 0..N {
                if j == i {
                    continue;
                }
                let (dx, dy, dz) = (x[j] - x[i], y[j] - y[i], z[j] - z[i]);
                let d2 = dx * dx + dy * dy + dz * dz;
                let mag = m[j] / (d2 * d2.sqrt());
                ax[i] += dx * mag;
                ay[i] += dy * mag;
                az[i] += dz * mag;
            }
        }
        for i in 0..N {
            vx[i] += 0.01 * ax[i];
            vy[i] += 0.01 * ay[i];
            vz[i] += 0.01 * az[i];
            x[i] += 0.01 * vx[i];
            y[i] += 0.01 * vy[i];
            z[i] += 0.01 * vz[i];
        }
    }
    println!("{:.9}", energy(&x, &y, &z, &vx, &vy, &vz, &m));
}
