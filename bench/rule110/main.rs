// rule110: the elementary cellular automaton Rule 110 on a ring of 500,000
// cells (start from splitmix64) for 1,000 generations. Prints the number of
// live cells at the end and the sum of their positions.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = 500_000usize;
    let mut c: Vec<u8> = (0..n as i64).map(|i| (mix(i) % 3 == 0) as u8).collect();
    let mut d = vec![0u8; n];
    for _ in 0..1000 {
        for i in 0..n {
            d[i] = (110u32 >> (c[(i + n - 1) % n] * 4 + c[i] * 2 + c[(i + 1) % n])) as u8 & 1;
        }
        std::mem::swap(&mut c, &mut d);
    }
    let live: i64 = c.iter().map(|&x| x as i64).sum();
    let pos: i64 = c.iter().enumerate().map(|(i, &x)| x as i64 * i as i64).sum();
    println!("{} {}", live, pos);
}
