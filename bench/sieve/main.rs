// The primes below 100,000,000 by a segmented sieve (segments of 262,144):
// their count and the largest.
fn main() {
    let (n, size) = (100_000_000i64, 262_144i64);
    let ps: Vec<i64> = (2..=10_000i64).filter(|&k| (2..).take_while(|d| d * d <= k).all(|d| k % d != 0)).collect();
    let mut f = vec![0u8; size as usize];
    let (mut count, mut largest) = (0i64, -1i64);
    let mut lo = 0;
    while lo < n {
        let hi = (lo + size).min(n);
        f.iter_mut().for_each(|x| *x = 1);
        for &p in ps.iter().take_while(|&&p| p * p < hi) {
            let mut m = ((lo + p - 1) / p * p).max(p * p);
            while m < hi {
                f[(m - lo) as usize] = 0;
                m += p;
            }
        }
        for j in 0..hi - lo {
            if f[j as usize] == 1 && lo + j >= 2 {
                count += 1;
                largest = lo + j;
            }
        }
        lo += size;
    }
    println!("{count} {largest}");
}
