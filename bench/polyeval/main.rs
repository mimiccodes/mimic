// polyeval: 1,000,000 polynomials of degree 31 modulo 1,000,000,007
// (coefficients from splitmix64), each evaluated at its own point by
// Horner's rule. Prints the sum of the values modulo p and their exclusive
// or.

const P: i64 = 1_000_000_007;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (mut s, mut x) = (0i64, 0i64);
    for k in 0..1_000_000i64 {
        let at = mix(100_000_000 + k) % P;
        let v = (0..32).fold(0i64, |v, j| (v * at % P + mix(32 * k + j) % P) % P);
        s = (s + v) % P;
        x ^= v;
    }
    println!("{} {}", s, x);
}
