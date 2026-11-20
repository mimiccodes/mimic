// tsp: the shortest round trip through 20 cities (from splitmix64,
// Manhattan distances) by the Held-Karp dynamic programme over the sets of
// cities in increasing order. Prints the length of the shortest tour and
// the sum of dp over the full set.

const N: usize = 20;
const M: usize = N - 1;
const INF: i64 = 1 << 50;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let xs: Vec<i64> = (0..N as i64).map(|c| mix(2 * c) % 1000).collect();
    let ys: Vec<i64> = (0..N as i64).map(|c| mix(2 * c + 1) % 1000).collect();
    let d = |a: usize, b: usize| (xs[a] - xs[b]).abs() + (ys[a] - ys[b]).abs();
    let mut dp = vec![INF; (1 << M) * M];
    for j in 0..M {
        dp[(1 << j) * M + j] = d(0, j + 1);
    }
    for s in 1usize..1 << M {
        for j in 0..M {
            if s >> j & 1 == 0 || s == 1 << j {
                continue;
            }
            let prev = s ^ (1 << j);
            let mut b = INF;
            for k in 0..M {
                if prev >> k & 1 == 1 {
                    b = b.min(dp[prev * M + k] + d(k + 1, j + 1));
                }
            }
            dp[s * M + j] = b;
        }
    }
    let full = (1 << M) - 1;
    let best = (0..M).map(|j| dp[full * M + j] + d(j + 1, 0)).min().unwrap();
    let sum: i64 = (0..M).map(|j| dp[full * M + j]).sum();
    println!("{} {}", best, sum);
}
