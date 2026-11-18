// The product of two polynomials of 524,288 coefficients (0 to 99, from
// splitmix64) by a radix-2 FFT of 2^20 points, rounded back to whole
// numbers: the sum of its coefficients, a checksum and the middle one.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn transform(re: &mut [f64], im: &mut [f64], tr: &[f64], ti: &[f64], sign: f64) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut h = 1;
    while h < n {
        let stride = n / (2 * h);
        for s in (0..n).step_by(2 * h) {
            for k in 0..h {
                let (wr, wi) = (tr[k * stride], sign * ti[k * stride]);
                let (a, b) = (s + k, s + k + h);
                let (xr, xi) = (wr * re[b] - wi * im[b], wr * im[b] + wi * re[b]);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        h *= 2;
    }
}

fn main() {
    let n = 1usize << 20;
    let half = n / 2;
    let tr: Vec<f64> = (0..half).map(|k| (2.0 * std::f64::consts::PI * k as f64 / n as f64).cos()).collect();
    let ti: Vec<f64> = (0..half).map(|k| (2.0 * std::f64::consts::PI * k as f64 / n as f64).sin()).collect();
    let (mut ar, mut ai, mut br, mut bi) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    for i in 0..half {
        ar[i] = (mix(i as u64) % 100) as f64;
        br[i] = (mix((n + i) as u64) % 100) as f64;
    }
    transform(&mut ar, &mut ai, &tr, &ti, -1.0);
    transform(&mut br, &mut bi, &tr, &ti, -1.0);
    for p in 0..n {
        let (r, i) = (ar[p] * br[p] - ai[p] * bi[p], ar[p] * bi[p] + ai[p] * br[p]);
        ar[p] = r;
        ai[p] = i;
    }
    transform(&mut ar, &mut ai, &tr, &ti, 1.0);
    let (mut sum, mut check, mut mid) = (0i64, 0i64, 0i64);
    for (i, &x) in ar.iter().enumerate() {
        let c = (x / n as f64).round() as i64;
        sum += c;
        check += c % 1000 * (i % 1000) as i64;
        if i == half - 1 {
            mid = c;
        }
    }
    println!("{sum} {check} {mid}");
}
