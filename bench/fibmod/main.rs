// fibmod: F(n) modulo 1,000,000,007 for 500,000 values n below 10^18
// (from splitmix64), by fast doubling over the bits of n. Prints the sum
// of the results modulo the prime, and their exclusive or.

const P: i64 = 1_000_000_007;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn fib(n: i64) -> i64 {
    let (mut a, mut b) = (0i64, 1i64);
    for bit in (0..=60).rev() {
        let c = a * ((2 * b - a + P) % P) % P;
        let d = (a * a + b * b) % P;
        if n >> bit & 1 == 1 {
            a = d;
            b = (c + d) % P;
        } else {
            a = c;
            b = d;
        }
    }
    a
}

fn main() {
    let (mut s, mut x) = (0i64, 0i64);
    for k in 0..500_000 {
        let f = fib(mix(k) % 1_000_000_000_000_000_000);
        s = (s + f) % P;
        x ^= f;
    }
    println!("{} {}", s, x);
}
