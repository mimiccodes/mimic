// ising: the 2D Ising model on a 512 × 512 torus at β = 0.44, 100 sweeps of
// Metropolis updates in checkerboard order, with random numbers from
// splitmix64 and the acceptance thresholds fixed as integers. Prints the
// magnetisation and the energy at the end.

const L: i64 = 512;
const N: i64 = L * L;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut s: Vec<i64> = (0..N).map(|i| if mix(i) % 2 == 1 { 1 } else { -1 }).collect();
    let at = |s: &[i64], x: i64, y: i64| s[(((y + L) % L) * L + (x + L) % L) as usize];
    for h in 0..200 {
        for i in 0..N {
            let (x, y) = (i % L, i / L);
            if (x + y) % 2 != h % 2 {
                continue;
            }
            let v = s[i as usize];
            let de = 2 * v * (at(&s, x - 1, y) + at(&s, x + 1, y) + at(&s, x, y - 1) + at(&s, x, y + 1));
            let r = mix((1 << 30) + h * N + i) % (1 << 32);
            let t = match de {
                4 => 738927063,
                8 => 127128606,
                _ => 0,
            };
            if de <= 0 || r < t {
                s[i as usize] = -v;
            }
        }
    }
    let m: i64 = s.iter().sum();
    let bonds: i64 = (0..N).map(|i| s[i as usize] * (at(&s, i % L + 1, i / L) + at(&s, i % L, i / L + 1))).sum();
    println!("{} {}", m, -bonds);
}
