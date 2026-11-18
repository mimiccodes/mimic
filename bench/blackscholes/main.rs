// 20,000,000 European options priced by Black-Scholes (the normal
// distribution as in Abramowitz and Stegun 26.2.17), inputs from
// splitmix64: the total of every call and put, each rounded to cents.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn unit(v: u64) -> f64 {
    (mix(v) >> 10) as f64 / 9007199254740992.0
}

fn cnd(x: f64) -> f64 {
    let k = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = k * (0.319381530 + k * (-0.356563782 + k * (1.781477937 + k * (-1.821255978 + k * 1.330274429))));
    let w = 1.0 - 0.3989422804014327 * (-0.5 * x * x).exp() * poly;
    if x < 0.0 { 1.0 - w } else { w }
}

fn cents(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

fn main() {
    let n = 20_000_000u64;
    let total: i64 = (0..n)
        .map(|i| {
            let (s, k) = (50.0 + 100.0 * unit(5 * i), 50.0 + 100.0 * unit(5 * i + 1));
            let (t, r, v) = (0.1 + 2.0 * unit(5 * i + 2), 0.01 + 0.05 * unit(5 * i + 3), 0.1 + 0.5 * unit(5 * i + 4));
            let root = v * t.sqrt();
            let d1 = ((s / k).ln() + (r + 0.5 * v * v) * t) / root;
            let d2 = d1 - root;
            let far = k * (-r * t).exp();
            cents(s * cnd(d1) - far * cnd(d2)) + cents(far * cnd(-d2) - s * cnd(-d1))
        })
        .sum();
    println!("{n} {total}");
}
