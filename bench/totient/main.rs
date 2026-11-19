// totient: Euler's phi for every number up to 10,000,000 by a sieve.
// Prints the sum of phi over 1 … n, how many n have phi(n) = n - 1, and
// phi(10,000,000).

fn main() {
    let n = 10_000_000usize;
    let mut phi: Vec<i64> = (0..=n as i64).collect();
    for p in 2..=n {
        if phi[p] == p as i64 {
            let mut m = p;
            while m <= n {
                phi[m] -= phi[m] / p as i64;
                m += p;
            }
        }
    }
    let sum: i64 = phi[1..].iter().sum();
    let primes = (2..=n).filter(|&k| phi[k] == k as i64 - 1).count();
    println!("{} {} {}", sum, primes, phi[n]);
}
