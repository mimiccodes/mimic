// π from 200,000,000 points of the unit square (splitmix64 coordinates):
// the count inside the quarter circle, and 4 × count / points.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let n = 200_000_000u64;
    let count = (0..n)
        .filter(|&i| {
            let x = (mix(2 * i) >> 10) as f64 / 9007199254740992.0;
            let y = (mix(2 * i + 1) >> 10) as f64 / 9007199254740992.0;
            x * x + y * y < 1.0
        })
        .count() as i64;
    let micro = count * 4_000_000 / n as i64;
    println!("{count} {}.{:06}", micro / 1_000_000, micro % 1_000_000);
}
