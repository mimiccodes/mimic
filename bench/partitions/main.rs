// partitions: the partition numbers p(n) modulo 1,000,000,007 for n up to
// 200,000, by Euler's pentagonal number recurrence. Prints p(200,000) and
// the sum of all p(n) modulo the prime.

const P: i64 = 1_000_000_007;

fn main() {
    let n = 200_000usize;
    let mut p = vec![0i64; n + 1];
    p[0] = 1;
    for m in 1..=n {
        let mut s = 0i64;
        let mut k = 1usize;
        loop {
            let g1 = k * (3 * k - 1) / 2;
            if g1 > m {
                break;
            }
            let g2 = k * (3 * k + 1) / 2;
            let t = p[m - g1] + if g2 <= m { p[m - g2] } else { 0 };
            s += if k % 2 == 1 { t } else { -t };
            k += 1;
        }
        p[m] = s.rem_euclid(P);
    }
    println!("{} {}", p[n], p.iter().fold(0, |a, &x| (a + x) % P));
}
