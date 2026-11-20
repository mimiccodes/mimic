// The product of two 120,000-digit numbers (30,000 limbs of four decimal
// digits, from splitmix64) by schoolbook multiplication: the product's
// limbs, their sum, the top limb and the middle one.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let n = 30_000usize;
    let limb = |seed: usize, i: usize| -> i64 { if i == n - 1 { 1 + (mix((seed + i) as u64) % 9999) as i64 } else { (mix((seed + i) as u64) % 10000) as i64 } };
    let a: Vec<i64> = (0..n).map(|i| limb(0, i)).collect();
    let b: Vec<i64> = (0..n).map(|i| limb(n, i)).collect();
    let mut c = vec![0i64; 2 * n];
    for i in 0..n {
        for j in 0..n {
            c[i + j] += a[i] * b[j];
        }
    }
    for k in 0..2 * n - 1 {
        c[k + 1] += c[k] / 10000;
        c[k] %= 10000;
    }
    let size = if c[2 * n - 1] == 0 { 2 * n - 1 } else { 2 * n };
    let sum: i64 = c[..size].iter().sum();
    println!("{size} {sum} {} {}", c[size - 1], c[n]);
}
