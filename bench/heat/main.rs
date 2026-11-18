// Heat over a 1,024 × 1,024 plate for 400 steps (each inner cell: itself
// weighted 4 and its four neighbours, over 8; the edge stays), starting
// from splitmix64: the sum of the cells and the centre, in thousandths.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let (n, steps) = (1024usize, 400);
    let mut g: Vec<f64> = (0..n * n).map(|p| (mix(p as u64) % 100000) as f64 / 1000.0).collect();
    let mut h = vec![0.0f64; n * n];
    for _ in 0..steps {
        for r in 0..n {
            for c in 0..n {
                let p = r * n + c;
                h[p] = if r == 0 || c == 0 || r == n - 1 || c == n - 1 {
                    g[p]
                } else {
                    let x = g[p];
                    (x + x + x + x + g[p - n] + g[p + n] + g[p - 1] + g[p + 1]) * 0.125
                };
            }
        }
        std::mem::swap(&mut g, &mut h);
    }
    let total: i64 = g.iter().map(|&x| (x * 1000.0) as i64).sum();
    println!("{total} {}", (g[n * (n / 2) + n / 2] * 1000.0) as i64);
}
