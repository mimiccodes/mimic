// bitpack: 8,000,000 values of 13 bits (from splitmix64) packed into 64-bit
// words, low bits first, then unpacked again. Prints the number of words,
// the sum of every word modulo 1,000,003, and 1 when unpacking gives the
// values back.

const N: usize = 8_000_000;
const B: usize = 13;

fn mix(v: i64) -> u64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let nw = (N * B + 63) / 64;
    let vals: Vec<u64> = (0..N as i64).map(|k| mix(k) % (1 << B)).collect();
    let mut words = vec![0u64; nw + 1];
    // a bit buffer: whole words go out as they fill
    let (mut buf, mut fill, mut w) = (0u64, 0usize, 0usize);
    for &v in &vals {
        buf |= v << fill;
        if fill + B >= 64 {
            words[w] = buf;
            w += 1;
            buf = if fill > 0 { v >> (64 - fill) } else { 0 };
            fill = fill + B - 64;
        } else {
            fill += B;
        }
    }
    if fill > 0 {
        words[w] = buf;
    }
    let check: u64 = words[..nw].iter().map(|w| w % 1_000_003).sum();
    let same = (0..N).all(|k| {
        let p = k * B;
        let lo = words[p / 64] >> (p % 64);
        let hi = if p % 64 + B > 64 { words[p / 64 + 1] << (64 - p % 64) } else { 0 };
        (lo | hi) & ((1 << B) - 1) == vals[k]
    });
    println!("{} {} {}", nw, check, same as i32);
}
