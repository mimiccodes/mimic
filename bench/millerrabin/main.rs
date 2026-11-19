// millerrabin: how many of the 3,000,000 odd numbers just below 2^31 are
// prime, by the deterministic Miller-Rabin test with bases 2, 3, 5 and 7,
// and their sum.

fn power(mut b: i64, mut e: i64, m: i64) -> i64 {
    let mut r = 1;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = r * b % m;
        }
        b = b * b % m;
        e >>= 1;
    }
    r
}

fn prime(n: i64) -> bool {
    let (mut d, mut s) = (n - 1, 0);
    while d % 2 == 0 {
        d /= 2;
        s += 1;
    }
    [2, 3, 5, 7].iter().all(|&a| {
        let mut x = power(a, d, n);
        if x == 1 || x == n - 1 {
            return true;
        }
        for _ in 1..s {
            x = x * x % n;
            if x == n - 1 {
                return true;
            }
        }
        false
    })
}

fn main() {
    let top = (1i64 << 31) - 1;
    let ps: Vec<i64> = (0..3_000_000).map(|k| top - 2 * k).filter(|&n| prime(n)).collect();
    println!("{} {}", ps.len(), ps.iter().sum::<i64>());
}
