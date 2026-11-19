// manacher: Manacher's algorithm over 2,000,000 letters from a 2-letter
// alphabet (from splitmix64), on the letters with a separator between each
// two. Prints the number of palindromic substrings, the longest one's
// length, and where the first of that length starts.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = 2_000_000usize;
    let m = 2 * n + 1;
    let t: Vec<u8> = (0..m).map(|i| if i % 2 == 1 { (mix((i / 2 / 2) as i64) % 2) as u8 } else { 2 }).collect();
    let mut p = vec![0usize; m];
    let (mut c, mut r) = (0usize, 0usize);
    for i in 0..m {
        let mut k = if i < r { (r - i).min(p[2 * c - i]) } else { 0 };
        while i >= k + 1 && i + k + 1 < m && t[i - k - 1] == t[i + k + 1] {
            k += 1;
        }
        p[i] = k;
        if i + k > r {
            c = i;
            r = i + k;
        }
    }
    let best = *p.iter().max().unwrap();
    let at = p.iter().position(|&x| x == best).unwrap();
    println!("{} {} {}", p.iter().map(|x| (x + 1) / 2).sum::<usize>(), best, (at - best) / 2);
}
