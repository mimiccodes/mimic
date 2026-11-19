// shuffle: the numbers 0 … 9,999,999 shuffled by Fisher-Yates (swaps from
// splitmix64). Prints the first and the last number and a
// position-weighted checksum.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = 10_000_000usize;
    let mut a: Vec<i64> = (0..n as i64).collect();
    for i in (1..n).rev() {
        let j = (mix(i as i64) % (i as i64 + 1)) as usize;
        a.swap(i, j);
    }
    let check: i64 = a.iter().enumerate().map(|(i, &x)| x * (i as i64 % 251 + 1)).sum();
    println!("{} {} {}", a[0], a[n - 1], check);
}
