// radix: 8,000,000 32-bit keys (from splitmix64) sorted by LSD radix sort,
// four passes of 8 bits (count, running offsets, scatter). Prints the
// smallest, the median and the largest, and a position-weighted checksum.

const N: usize = 8_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut a: Vec<i64> = (0..N as i64).map(|k| mix(k) % 4294967296).collect();
    let mut b = vec![0i64; N];
    for s in (0..32).step_by(8) {
        let mut pos = [0usize; 256];
        for &x in &a {
            pos[(x >> s & 255) as usize] += 1;
        }
        let mut run = 0;
        for p in pos.iter_mut() {
            let c = *p;
            *p = run;
            run += c;
        }
        for &x in &a {
            let d = (x >> s & 255) as usize;
            b[pos[d]] = x;
            pos[d] += 1;
        }
        std::mem::swap(&mut a, &mut b);
    }
    let check: i64 = a.iter().enumerate().map(|(i, &x)| x * (i as i64 % 251 + 1)).sum();
    println!("{} {} {} {}", a[0], a[N / 2], a[N - 1], check);
}
