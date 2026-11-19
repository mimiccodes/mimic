// zalgo: the Z-array of 4,000,000 letters from a 4-letter alphabet (from
// splitmix64) by the Z-algorithm. Prints the sum of z, how many positions
// have z of 12 or more, and the largest z.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = 4_000_000usize;
    let s: Vec<u8> = (0..n as i64).map(|i| (mix(i / 3) % 4) as u8).collect();
    let mut z = vec![0usize; n];
    let (mut l, mut r) = (0usize, 0usize);
    for i in 1..n {
        let mut k = if i < r { (r - i).min(z[i - l]) } else { 0 };
        while i + k < n && s[k] == s[i + k] {
            k += 1;
        }
        z[i] = k;
        if i + k > r {
            l = i;
            r = i + k;
        }
    }
    println!("{} {} {}", z.iter().sum::<usize>(), z.iter().filter(|&&x| x >= 12).count(), z.iter().max().unwrap());
}
